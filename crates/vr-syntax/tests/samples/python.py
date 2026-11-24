# Greets the user.
import sys


class Greeter:
    def __init__(self, name: str) -> None:
        self.name = name

    def greet(self) -> str:
        if not self.name:
            return "Hello, world"
        return f"Hello, {self.name}"


def main() -> int:
    print(Greeter(sys.argv[0]).greet())
    return 0
