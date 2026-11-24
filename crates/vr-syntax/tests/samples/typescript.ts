// A user record.
interface User {
  name: string;
  age: number;
}

export function greet(user: User): string {
  const prefix = "Hello, ";
  return prefix + user.name;
}

type Id = string | number;
let id: Id = 42;
