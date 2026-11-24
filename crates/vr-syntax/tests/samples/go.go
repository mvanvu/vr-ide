// Package main greets the user.
package main

import "fmt"

func greet(name string) string {
	if name == "" {
		return "Hello, world"
	}
	return fmt.Sprintf("Hello, %s", name)
}

func main() {
	fmt.Println(greet("vr"))
}
