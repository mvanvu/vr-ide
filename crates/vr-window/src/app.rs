//! The window runner: a winit [`ApplicationHandler`] that owns the main thread, the window, a
//! softbuffer surface and the ratatui terminal over a [`PixelBackend`]. It knows nothing about
//! the app: window events become crossterm events, handed in batches to a [`WindowHandler`]
//! (vr-ide's loop step); title, cursor, IME area and attention come back through [`WindowOps`].
//!
//! Frame pacing: one step per event batch (`about_to_wait`), then a wait until the step's next
//! wake or the cursor's next blink (`ControlFlow::WaitUntil`); no busy loop. Only the damaged
//! areas are copied and presented when the surface still holds the previous frame.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::Event;
use ratatui::Terminal;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, UserAttentionType, Window, WindowId};

use crate::backend::{CursorShape, GridLayout, PixelBackend};
use crate::canvas::PixelRect;
use crate::font::{CellMetrics, FontSet};
use crate::input::{ImeInput, KeyInput, MouseInput};
use crate::palette::Palette;

/// How long a blinking cursor stays shown, then hidden.
pub const BLINK: Duration = Duration::from_millis(530);

/// What the window starts with.
#[derive(Debug)]
pub struct WindowOptions {
    pub title: String,
    /// Wayland app id / X11 `WM_CLASS`, so the desktop groups the window with its launcher.
    pub app_id: String,
    /// Loaded before the window exists (slow); its scale is replaced by the window's.
    pub fonts: FontSet,
    pub palette: Palette,
    /// The initial grid in cells.
    pub cols: u16,
    pub rows: u16,
}

/// The window could not be shown; nothing was handed to the [`WindowHandler`], so the caller can
/// still fall back to the terminal.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("cannot connect to the display: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error("cannot open a window: {0}")]
    Window(#[from] winit::error::OsError),
    #[error("cannot draw into the window: {0}")]
    Surface(String),
}

/// What a step tells the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// Close the window and return from [`run`].
    pub quit: bool,
    /// Step again within this time even without events.
    pub next_wake: Duration,
}

/// What the app may change on the window between frames.
pub trait WindowOps {
    fn set_title(&mut self, title: &str);
    /// The text cursor the backend draws.
    fn set_cursor(&mut self, shape: CursorShape, blinking: bool);
    /// The cell where the IME puts its candidate window (`None`: no cursor shown).
    fn set_ime_cell(&mut self, cell: Option<(u16, u16)>);
    /// Asks the desktop for attention (taskbar hint), only while the window is not focused.
    fn request_attention(&mut self);
}

/// The app side of the window: vr-ide's loop.
pub trait WindowHandler {
    /// One turn: handle `events` (in order), run background work, draw through `terminal`.
    fn step(
        &mut self,
        terminal: &mut Terminal<PixelBackend>,
        ops: &mut dyn WindowOps,
        events: Vec<Event>,
        now: Instant,
    ) -> Step;
    /// The close button (or the window manager) asked the window to close. The app decides:
    /// the next step's result says whether it quits.
    fn close_requested(&mut self);
    /// Presenting failed or the display went away after the start; the runner stops after this
    /// call and [`run`] returns `Ok`, so the app ends the session itself.
    fn failed(&mut self, error: String);
}

/// Opens the window and runs until a step quits (or presenting or the display fails). Returns
/// `Err` only when the window never started, before the handler saw anything.
pub fn run(options: WindowOptions, handler: &mut impl WindowHandler) -> Result<(), StartError> {
    let event_loop = EventLoop::new()?;
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    {
        use winit::platform::x11::EventLoopExtX11;
        if event_loop.is_x11() {
            x11::install();
        }
    }
    let mut runner = Runner {
        handler,
        options: Some(options),
        live: None,
        start_error: None,
        started: false,
    };
    let result = event_loop.run_app(&mut runner);
    runner.finish(result)
}

struct Runner<'h, H: WindowHandler> {
    handler: &'h mut H,
    options: Option<WindowOptions>,
    live: Option<Live>,
    start_error: Option<StartError>,
    /// The window showed: from here on the handler owns the session and its exit.
    started: bool,
}

type Surface = softbuffer::Surface<Arc<Window>, Arc<Window>>;

struct Live {
    window: Arc<Window>,
    surface: Surface,
    surface_size: (u32, u32),
    terminal: Terminal<PixelBackend>,
    control: Control,
    mods: ModifiersState,
    ime: ImeInput,
    mouse: MouseInput,
    events: Vec<Event>,
    scale: f64,
    blink: Blink,
    /// Damage not presented yet, and the damage of the last presented frame (for a surface
    /// that hands back the frame before last).
    damage: Vec<PixelRect>,
    last_damage: Vec<PixelRect>,
    /// The compositor asked for the whole window (exposed, first frame).
    full_present: bool,
    next_wake: Instant,
}

/// [`WindowOps`] over the real window.
struct Control {
    window: Arc<Window>,
    focused: bool,
    layout: GridLayout,
    cursor: (CursorShape, bool),
    ime: ImePlacement,
}

impl WindowOps for Control {
    fn set_title(&mut self, title: &str) {
        self.window.set_title(title);
    }

    fn set_cursor(&mut self, shape: CursorShape, blinking: bool) {
        self.cursor = (shape, blinking);
    }

    fn set_ime_cell(&mut self, cell: Option<(u16, u16)>) {
        self.ime.set_cell(cell);
    }

    fn request_attention(&mut self) {
        if !self.focused {
            self.window
                .request_user_attention(Some(UserAttentionType::Informational));
        }
    }
}

impl Control {
    /// Places the IME candidate window at the app's cursor cell, in the current layout (a font
    /// or scale change moves the same cell).
    fn place_ime(&mut self, layout: GridLayout) {
        self.layout = layout;
        if let Some(r) = self.ime.take(layout) {
            self.window.set_ime_cursor_area(
                PhysicalPosition::new(r.x, r.y),
                PhysicalSize::new(r.width, r.height),
            );
        }
    }
}

/// Where the IME candidate window goes, and whether the window must be told (again).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ImePlacement {
    cell: Option<(u16, u16)>,
    layout: Option<GridLayout>,
    pending: bool,
}

impl ImePlacement {
    /// The app's cursor cell (`None`: no cursor shown).
    pub fn set_cell(&mut self, cell: Option<(u16, u16)>) {
        self.cell = cell;
        self.pending = true;
    }

    /// The window's IME state started over (focus, IME enabled, a cancelled composition): the
    /// compositor no longer knows the area, so it is sent again.
    pub fn reset(&mut self) {
        self.pending = true;
    }

    /// The area to send now, in `layout` (a font or scale change moves the same cell); `None`
    /// when the window already has it or no cursor is shown.
    pub fn take(&mut self, layout: GridLayout) -> Option<PixelRect> {
        if self.layout != Some(layout) {
            self.layout = Some(layout);
            self.pending = true;
        }
        if !std::mem::take(&mut self.pending) {
            return None;
        }
        self.cell.map(|(col, row)| ime_area(&layout, col, row))
    }
}

/// Whether `event` starts the window's IME state over. On Wayland the text input is created
/// (or enabled) on keyboard focus and forgets any area set before.
#[must_use]
pub fn resets_ime(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::Focused(true) | WindowEvent::Ime(winit::event::Ime::Enabled)
    )
}

impl<H: WindowHandler> Runner<'_, H> {
    /// What [`run`] returns: a start error only while the handler has seen nothing. After the
    /// start, a failing event loop (the display went away) goes to the handler, which ends the
    /// session its own way (recovery, session save).
    fn finish(
        &mut self,
        result: Result<(), winit::error::EventLoopError>,
    ) -> Result<(), StartError> {
        if let Some(e) = self.start_error.take() {
            return Err(e);
        }
        match result {
            Ok(()) => Ok(()),
            Err(e) if self.started => {
                self.handler
                    .failed(format!("the connection to the display was lost: {e}"));
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<Live, StartError> {
        let Some(options) = self.options.take() else {
            return Err(StartError::Surface("started twice".into()));
        };
        let size = initial_size(options.fonts.metrics(), options.cols, options.rows);
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(options.title)
            .with_inner_size(size);
        #[cfg(all(
            unix,
            not(any(target_os = "macos", target_os = "ios", target_os = "android"))
        ))]
        {
            use winit::platform::wayland::WindowAttributesExtWayland;
            // One field shared by X11 (WM_CLASS) and Wayland (app id).
            attrs = attrs.with_name(options.app_id.clone(), options.app_id.clone());
        }
        let _ = &options.app_id;
        let window = Arc::new(event_loop.create_window(attrs)?);
        let context = softbuffer::Context::new(window.clone())
            .map_err(|e| StartError::Surface(e.to_string()))?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|e| StartError::Surface(e.to_string()))?;
        window.set_ime_allowed(true);
        window.set_cursor(CursorIcon::Text);
        let scale = window.scale_factor();
        let mut fonts = options.fonts;
        let _ = fonts.set_scale(scale as f32);
        let px = window.inner_size();
        let backend = PixelBackend::new(fonts, options.palette, px.width.max(1), px.height.max(1));
        let layout = backend.layout();
        let terminal = Terminal::new(backend).map_err(|e| StartError::Surface(e.to_string()))?;
        Ok(Live {
            control: Control {
                window: window.clone(),
                focused: window.has_focus(),
                layout,
                cursor: (CursorShape::Block, false),
                ime: ImePlacement::default(),
            },
            window,
            surface,
            surface_size: (0, 0),
            terminal,
            mods: ModifiersState::empty(),
            ime: ImeInput::new(),
            mouse: MouseInput::new(),
            events: Vec::new(),
            scale,
            blink: Blink::default(),
            damage: Vec::new(),
            last_damage: Vec::new(),
            full_present: true,
            next_wake: Instant::now(),
        })
    }
}

impl<H: WindowHandler> ApplicationHandler for Runner<'_, H> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.live.is_some() || self.start_error.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(live) => {
                self.live = Some(live);
                self.started = true;
            }
            Err(e) => {
                self.start_error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if resets_ime(&event) {
            live.control.ime.reset();
        }
        match event {
            WindowEvent::CloseRequested => self.handler.close_requested(),
            WindowEvent::Resized(size) => live.relayout(size),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                live.scale = scale_factor;
                let size = live.window.inner_size();
                live.relayout(size);
            }
            WindowEvent::RedrawRequested => live.full_present = true,
            WindowEvent::Focused(focused) => {
                live.control.focused = focused;
                if !focused {
                    live.mouse.focus_lost();
                }
                live.blink.reset(Instant::now());
                live.events.push(if focused {
                    Event::FocusGained
                } else {
                    Event::FocusLost
                });
            }
            WindowEvent::ModifiersChanged(m) => live.mods = m.state(),
            // Keys the window was given while focused elsewhere (X11 sends them on focus).
            WindowEvent::KeyboardInput {
                is_synthetic: true, ..
            } => {}
            WindowEvent::KeyboardInput { event, .. } => {
                let keys = live.ime.key(&KeyInput::from(&event), live.mods);
                live.events.extend(keys.into_iter().map(Event::Key));
                if live.ime.take_cancel_request() {
                    // Esc dropped the composition: the IME must forget it too.
                    live.window.set_ime_allowed(false);
                    live.window.set_ime_allowed(true);
                    live.control.ime.reset();
                }
                live.blink.reset(Instant::now());
            }
            WindowEvent::Ime(ime) => {
                let keys = live.ime.ime(&ime);
                live.events.extend(keys.into_iter().map(Event::Key));
                live.blink.reset(Instant::now());
            }
            WindowEvent::CursorMoved { position, .. } => {
                let layout = live.control.layout;
                if let Some(m) = live.mouse.moved(&layout, position.x, position.y, live.mods) {
                    live.events.push(Event::Mouse(m));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(m) = live.mouse.button(button, state, live.mods) {
                    live.events.push(Event::Mouse(m));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let layout = live.control.layout;
                let wheel = live.mouse.wheel(&layout, delta, live.mods);
                live.events.extend(wheel.into_iter().map(Event::Mouse));
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let now = Instant::now();
        let events = std::mem::take(&mut live.events);
        let step = self
            .handler
            .step(&mut live.terminal, &mut live.control, events, now);
        live.next_wake = now + step.next_wake;
        let layout = live.terminal.backend().layout();
        live.control.place_ime(layout);
        live.overlays(now);
        if let Err(e) = live.present() {
            self.handler
                .failed(format!("drawing into the window failed: {e}"));
            event_loop.exit();
            return;
        }
        if step.quit {
            event_loop.exit();
            return;
        }
        let wake = live
            .blink
            .next()
            .map_or(live.next_wake, |b| b.min(live.next_wake));
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

impl Live {
    /// A new window size or scale: the grid is laid out again and the app gets a resize.
    fn relayout(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return; // minimised: keep the grid
        }
        let backend = self.terminal.backend_mut();
        // A bad scale keeps the old font; the grid still follows the window.
        let _ = backend.resize(size.width, size.height, self.scale as f32);
        let layout = backend.layout();
        self.control.layout = layout;
        self.events.push(Event::Resize(layout.cols, layout.rows));
        self.full_present = true;
    }

    /// Cursor shape and blink phase, the IME preedit at the cursor; collects the damage.
    fn overlays(&mut self, now: Instant) {
        let (shape, blinking) = self.control.cursor;
        self.blink.set(blinking && self.control.focused, now);
        self.blink.tick(now);
        let preedit = self.ime.preedit().map(|p| {
            let at = self.terminal.backend().cursor_position();
            p.at(at.x, at.y)
        });
        let backend = self.terminal.backend_mut();
        backend.set_cursor_shape(shape);
        backend.set_cursor_blink_on(self.blink.on);
        backend.set_preedit(preedit);
        backend.render_overlays();
        self.damage.extend(backend.take_damage());
    }

    fn present(&mut self) -> Result<(), String> {
        if self.damage.is_empty() && !self.full_present {
            return Ok(());
        }
        let canvas = self.terminal.backend().canvas();
        let (w, h) = (canvas.width(), canvas.height());
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
            return Ok(());
        };
        if self.surface_size != (w, h) {
            self.surface.resize(nw, nh).map_err(|e| e.to_string())?;
            self.surface_size = (w, h);
            self.full_present = true;
        }
        let mut buffer = self.surface.buffer_mut().map_err(|e| e.to_string())?;
        let plan = if self.full_present {
            None
        } else {
            copy_plan(buffer.age(), &self.damage, &self.last_damage)
        };
        self.window.pre_present_notify();
        match plan {
            Some(rects) => {
                for r in &rects {
                    canvas.copy_rect_to(*r, &mut buffer);
                }
                let damage: Vec<softbuffer::Rect> = rects.iter().filter_map(to_sb_rect).collect();
                buffer
                    .present_with_damage(&damage)
                    .map_err(|e| e.to_string())?;
            }
            None => {
                buffer.copy_from_slice(canvas.pixels());
                buffer.present().map_err(|e| e.to_string())?;
            }
        }
        self.last_damage = std::mem::take(&mut self.damage);
        if self.full_present {
            self.last_damage = vec![canvas.bounds()];
            self.full_present = false;
        }
        Ok(())
    }
}

fn to_sb_rect(r: &PixelRect) -> Option<softbuffer::Rect> {
    Some(softbuffer::Rect {
        x: r.x,
        y: r.y,
        width: NonZeroU32::new(r.width)?,
        height: NonZeroU32::new(r.height)?,
    })
}

/// The rects to copy into a surface buffer of `age` (softbuffer: frames since it was shown, 0 =
/// unknown contents); `None` means copy everything. A buffer from the frame before last also
/// misses the last frame's damage.
#[must_use]
pub fn copy_plan(age: u8, damage: &[PixelRect], last: &[PixelRect]) -> Option<Vec<PixelRect>> {
    match age {
        1 => Some(damage.to_vec()),
        2 => Some(damage.iter().chain(last).copied().collect()),
        _ => None,
    }
}

/// The window's inner size for `cols` x `rows` cells of a font laid out at scale 1.
#[must_use]
pub fn initial_size(cell: CellMetrics, cols: u16, rows: u16) -> LogicalSize<u32> {
    LogicalSize::new(
        u32::from(cols) * cell.width.max(1),
        u32::from(rows) * cell.height.max(1),
    )
}

/// The pixel area of a cell, where the IME puts its candidate window (below it).
#[must_use]
pub fn ime_area(layout: &GridLayout, col: u16, row: u16) -> PixelRect {
    layout.cell_rect(col.min(layout.cols.saturating_sub(1)), row, 1)
}

/// The blink phase of the text cursor: shown for [`BLINK`], hidden for [`BLINK`]; shown again
/// at once on input, so a moving cursor never disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blink {
    pub on: bool,
    /// When the phase flips; `None`: not blinking.
    flip: Option<Instant>,
}

impl Default for Blink {
    fn default() -> Self {
        Self {
            on: true,
            flip: None,
        }
    }
}

impl Blink {
    /// Starts or stops blinking; a steady cursor is always shown.
    pub fn set(&mut self, blinking: bool, now: Instant) {
        match (blinking, self.flip) {
            (true, None) => self.flip = Some(now + BLINK),
            (false, _) => *self = Self::default(),
            (true, Some(_)) => {}
        }
    }

    /// Input happened: shown, a full period before the next flip.
    pub fn reset(&mut self, now: Instant) {
        self.on = true;
        if self.flip.is_some() {
            self.flip = Some(now + BLINK);
        }
    }

    /// Flips the phase when its time came.
    pub fn tick(&mut self, now: Instant) {
        if let Some(at) = self.flip
            && now >= at
        {
            self.on = !self.on;
            self.flip = Some(now + BLINK);
        }
    }

    /// When the loop must wake to flip the phase.
    #[must_use]
    pub fn next(&self) -> Option<Instant> {
        self.flip
    }
}

/// Runs `hook` when the X11 connection breaks, just before libX11 ends the process (no-op off
/// X11). The hook runs inside libX11's error handler: it must not touch the app's state, only
/// what was published for a crash (recovery). `false` when a hook was registered before.
pub fn on_x11_connection_lost(hook: fn()) -> bool {
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    return x11::set_hook(hook);
    #[cfg(not(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    )))]
    {
        let _ = hook;
        false
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
mod x11;

#[cfg(test)]
mod tests;
