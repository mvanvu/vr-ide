// A simple greeter.
class Greeter {
  final String name;

  Greeter(this.name);

  String greet() {
    if (name.isEmpty) {
      return 'Hello, world';
    }
    return "Hello, $name";
  }
}

void main() {
  print(Greeter('vr').greet());
}
