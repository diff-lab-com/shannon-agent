// Fixture: tiny_go.go
// Smallest possible Go file with package-level function, method, struct,
// interface, type alias, and const block.

package tiny

// Add is a free function.
func Add(a, b int) int {
	return a + b
}

// Counter is a struct with methods.
type Counter struct {
	count int
}

func NewCounter(initial int) *Counter {
	return &Counter{count: initial}
}

func (c *Counter) Increment() {
	c.count++
}

func (c *Counter) Value() int {
	return c.count
}

// Greeter is an interface.
type Greeter interface {
	Greet() string
}

// Pair is a generic type alias.
type Pair[T any] struct {
	Left  T
	Right T
}

const DefaultLimit = 100
