// Adds two numbers.
export function add(a, b) {
  const label = "sum";
  return a + b;
}

class Counter {
  count = 0;
  increment() {
    this.count += 1;
  }
}

console.log(add(1, 2), new Counter());
