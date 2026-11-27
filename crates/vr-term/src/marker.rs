//! A byte-level matcher for one exact marker sequence in a PTY's output (SSH login, SEC-S6).
//!
//! The raw bytes pass through it before the emulator sees them: every occurrence of the marker
//! is removed, so it never reaches the grid, the scrollback or a selection. While the stream is
//! in the middle of a possible marker at most `marker.len() - 1` bytes are held back; they are
//! released, in order, as soon as a byte disproves the match.

pub(crate) struct MarkerScan {
    marker: Vec<u8>,
    /// How many leading bytes of the marker the end of the stream matches (held back).
    held: usize,
    /// `fail[n]`: the longest proper prefix of the marker that ends `marker[..n]`.
    fail: Vec<usize>,
}

impl MarkerScan {
    /// `None` for an empty marker (nothing to look for).
    pub fn new(marker: Vec<u8>) -> Option<MarkerScan> {
        if marker.is_empty() {
            return None;
        }
        let mut fail = vec![0usize; marker.len() + 1];
        let mut k = 0;
        for n in 1..marker.len() {
            while k > 0 && marker[n] != marker[k] {
                k = fail[k];
            }
            if marker[n] == marker[k] {
                k += 1;
            }
            fail[n + 1] = k;
        }
        Some(MarkerScan {
            marker,
            held: 0,
            fail,
        })
    }

    /// Appends the bytes of `input` that belong to no marker to `out`; returns how many
    /// markers ended in `input`.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> usize {
        let mut hits = 0;
        for &b in input {
            loop {
                if b == self.marker[self.held] {
                    self.held += 1;
                    if self.held == self.marker.len() {
                        hits += 1;
                        self.held = 0;
                    }
                    break;
                }
                if self.held == 0 {
                    out.push(b);
                    break;
                }
                // The held bytes are no marker: release those that cannot start one and try
                // this byte again after the shorter prefix that still can.
                let keep = self.fail[self.held];
                out.extend_from_slice(&self.marker[..self.held - keep]);
                self.held = keep;
            }
        }
        hits
    }

    /// The stream ended: the held bytes were no marker after all.
    pub fn flush(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.marker[..self.held]);
        self.held = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: &[u8] = b"\x1b]7717;vr-ide-ssh;00112233445566778899aabbccddeeff\x07";

    fn scan() -> MarkerScan {
        MarkerScan::new(MARKER.to_vec()).expect("a marker")
    }

    fn run(chunks: &[&[u8]]) -> (Vec<u8>, usize) {
        let mut s = scan();
        let (mut out, mut hits) = (Vec::new(), 0);
        for c in chunks {
            hits += s.feed(c, &mut out);
        }
        (out, hits)
    }

    #[test]
    fn an_empty_marker_is_no_scanner() {
        assert!(MarkerScan::new(Vec::new()).is_none());
    }

    #[test]
    fn the_marker_is_removed_whole_wherever_the_reads_split_it() {
        let stream = [b"before".as_slice(), MARKER, b"after"].concat();
        for cut in 0..=stream.len() {
            let (out, hits) = run(&[&stream[..cut], &stream[cut..]]);
            assert_eq!(hits, 1, "cut at {cut}");
            assert_eq!(out, b"beforeafter", "cut at {cut}");
        }
        // One byte per read.
        let bytes: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(run(&bytes), (b"beforeafter".to_vec(), 1));
    }

    #[test]
    fn every_occurrence_is_removed_and_counted() {
        let stream = [MARKER, b"x", MARKER, MARKER].concat();
        assert_eq!(run(&[&stream]), (b"x".to_vec(), 3));
    }

    #[test]
    fn another_nonce_or_shape_passes_through_unchanged() {
        let wrong = b"\x1b]7717;vr-ide-ssh;ffffffffffffffffffffffffffffffff\x07";
        let short = b"\x1b]7717;vr-ide-ssh;00112233445566778899aabbccddeef\x07";
        let other_osc = b"\x1b]7718;vr-ide-ssh;00112233445566778899aabbccddeeff\x07";
        let st_ended = b"\x1b]7717;vr-ide-ssh;00112233445566778899aabbccddeeff\x1b\\";
        for s in [wrong.as_slice(), short, other_osc, st_ended] {
            let mut sc = scan();
            let mut out = Vec::new();
            let hits = sc.feed(s, &mut out);
            sc.flush(&mut out);
            assert_eq!((out.as_slice(), hits), (s, 0));
        }
    }

    #[test]
    fn held_bytes_are_released_in_order_once_a_byte_disproves_the_match() {
        let mut s = scan();
        let mut out = Vec::new();
        assert_eq!(s.feed(b"a\x1b]7717;vr-ide-ss", &mut out), 0);
        assert_eq!(out, b"a", "the possible marker is held back");
        assert_eq!(s.feed(b"x", &mut out), 0);
        assert_eq!(out, b"a\x1b]7717;vr-ide-ssx");
    }

    #[test]
    fn at_most_the_marker_minus_one_byte_is_held() {
        let mut s = scan();
        let mut out = Vec::new();
        s.feed(&MARKER[..MARKER.len() - 1], &mut out);
        assert!(out.is_empty());
        s.flush(&mut out);
        assert_eq!(out, &MARKER[..MARKER.len() - 1]);
        // Flushed bytes are gone: the rest of a marker alone is no marker.
        assert_eq!(s.feed(&MARKER[MARKER.len() - 1..], &mut out), 0);
    }

    #[test]
    fn a_marker_that_starts_inside_a_failed_match_is_found() {
        // The failed match's tail is the start of a real marker.
        let stream = [&MARKER[..5], MARKER, b"z"].concat();
        assert_eq!(run(&[&stream]), ([&MARKER[..5], b"z"].concat(), 1));
        // A marker with a repeating prefix exercises the fallback table.
        let mut s = MarkerScan::new(b"aab".to_vec()).unwrap();
        let mut out = Vec::new();
        assert_eq!(s.feed(b"aaab-aaxaab", &mut out), 2);
        assert_eq!(out, b"a-aax");
    }
}
