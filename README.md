# pwn_arena

[![Crates.io](https://img.shields.io/crates/v/pwn_arena.svg)](https://crates.io/crates/pwn_arena)
[![Documentation](https://docs.rs/pwn_arena/badge.svg)](https://docs.rs/pwn_arena)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

A minimal, high-performance arena allocator for `no_std` environments with compile-time fixed capacity.

## Features

- 🚀 **Zero-cost abstractions**: Minimal runtime overhead
- 🔒 **No-std, no-alloc**: Perfect for embedded systems and kernels
- 📦 **Fixed-size**: All memory pre-allocated at compile time
- 🔄 **Generic**: Works with any `Copy` type
- 🛡️ **Type-safe**: Strongly-typed indices prevent mixing arenas
- 🔧 **Interior mutability**: Safe concurrent access via `RefCell`
- 📊 **Statistics**: Built-in fragmentation and usage tracking
- 🌲 **Tree support**: Recursive deletion and deep copying for tree-like structures
- ⚡ **Zero dependencies**: Only uses core library primitives

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
pwn_arena = "0.1.0"
```

## Basic Usage

```rust
use pwn_arena::{Arena, ArenaIndex};

// Create an arena with capacity for 1024 i32 values
let arena: Arena<i32, 1024> = Arena::new(0);

// Allocate some values
let idx1 = arena.alloc(42).unwrap();
let idx2 = arena.alloc(100).unwrap();

// Access values
assert_eq!(arena.get(idx1).unwrap(), 42);
assert_eq!(arena.get(idx2).unwrap(), 100);

// Modify values
arena.set(idx1, 99).unwrap();

// Free when done
arena.free(idx1).unwrap();
arena.free(idx2).unwrap();
```

## Advanced Usage

### Custom Types

```rust
use pwn_arena::Arena;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Vector3 {
    x: f32,
    y: f32,
    z: f32,
}

let arena: Arena<Vector3, 100> = Arena::new(Vector3 { x: 0.0, y: 0.0, z: 0.0 });

let pos = arena.alloc(Vector3 { x: 1.0, y: 2.0, z: 3.0 }).unwrap();
```

### Tree Structures

Build and manage tree-like data structures with recursive operations:

```rust
use pwn_arena::{Arena, ArenaIndex, ArenaDelete, ArenaCopy, ArenaResult};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tree {
    Leaf(i32),
    Branch(ArenaIndex, ArenaIndex),
}

// Implement recursive deletion
impl ArenaDelete<Tree, 1024> for Tree {
    fn delete_recursive(&self, arena: &Arena<Tree, 1024>) -> ArenaResult<()> {
        match *self {
            Tree::Leaf(_) => Ok(()),
            Tree::Branch(left, right) => {
                arena.delete_recursive(left)?;
                arena.delete_recursive(right)?;
                Ok(())
            }
        }
    }
}

// Implement deep copying
impl ArenaCopy<Tree, 1024> for Tree {
    fn copy_deep(&self, arena: &Arena<Tree, 1024>) -> ArenaResult<Tree> {
        match *self {
            Tree::Leaf(n) => Ok(Tree::Leaf(n)),
            Tree::Branch(left, right) => {
                let new_left = arena.copy_deep(left)?;
                let new_right = arena.copy_deep(right)?;
                Ok(Tree::Branch(new_left, new_right))
            }
        }
    }
}

// Usage
let arena: Arena<Tree, 1024> = Arena::new(Tree::Leaf(0));

let left = arena.alloc(Tree::Leaf(1)).unwrap();
let right = arena.alloc(Tree::Leaf(2)).unwrap();
let root = arena.alloc(Tree::Branch(left, right)).unwrap();

// Recursively delete entire tree
arena.delete_recursive(root).unwrap();

// Or create a deep copy
let copied_tree = arena.copy_deep(root).unwrap();
```

### Iteration

```rust
use pwn_arena::Arena;

let arena: Arena<i32, 10> = Arena::new(0);

arena.alloc(10).unwrap();
arena.alloc(20).unwrap();
arena.alloc(30).unwrap();

for (index, value) in arena.iter() {
    println!("Index: {:?}, Value: {}", index, value);
}
```

### Statistics

```rust
use pwn_arena::Arena;

let arena: Arena<i32, 100> = Arena::new(0);

for i in 0..25 {
    arena.alloc(i).unwrap();
}

let stats = arena.stats();
println!("Capacity: {}", stats.capacity);
println!("Allocated: {}", stats.allocated);
println!("Free: {}", stats.free);
println!("Usage: {:.2}%", stats.usage_percent());
println!("Fragmentation: {:.2}", stats.fragmentation);
```

## API Overview

### Core Methods

| Method | Description |
|--------|-------------|
| `new(default_value)` | Create a new arena |
| `alloc(value)` | Allocate a cell and return its index |
| `get(index)` | Get a copy of the value at an index |
| `set(index, value)` | Update the value at an index |
| `free(index)` | Free a cell for reuse |
| `clear()` | Free all cells at once |
| `is_allocated(index)` | Check if an index is currently allocated |

### Query Methods

| Method | Description |
|--------|-------------|
| `capacity()` | Get maximum capacity |
| `len()` | Get number of allocated cells |
| `is_empty()` | Check if no cells are allocated |
| `is_full()` | Check if all cells are allocated |
| `available()` | Get number of free cells |
| `iter()` | Iterate over allocated cells |
| `stats()` | Get detailed usage statistics |

### Advanced Methods

| Method | Description |
|--------|-------------|
| `delete_recursive(index)` | Recursively delete a tree structure |
| `copy_deep(index)` | Create a deep copy of a tree structure |

## Error Handling

All fallible operations return `ArenaResult<T>`:

```rust
use pwn_arena::{Arena, ArenaError};

let arena: Arena<i32, 3> = Arena::new(0);

// Fill the arena
arena.alloc(1).unwrap();
arena.alloc(2).unwrap();
arena.alloc(3).unwrap();

// This will fail
match arena.alloc(4) {
    Ok(_) => println!("Allocated successfully"),
    Err(ArenaError::OutOfMemory) => println!("Arena is full!"),
    Err(ArenaError::InvalidIndex) => println!("Invalid index"),
}
```

### Error Types

- `ArenaError::OutOfMemory` - Arena is full, cannot allocate
- `ArenaError::InvalidIndex` - Index is out of bounds or not allocated

## Memory Layout

For an `Arena<T, N>`:
- **Size**: `N * sizeof(T) + N * 1 + 8` bytes
- **Cells**: Fixed array of `T` values
- **Bitmap**: Boolean array tracking allocation status
- **Hint**: Single `usize` for allocation optimization

Example: `Arena<i32, 1000>` uses approximately **5,008 bytes** (4000 + 1000 + 8).

## Performance Characteristics

| Operation | Time Complexity | Notes |
|-----------|----------------|-------|
| `alloc()` | O(n) worst-case | O(1) amortized with hint |
| `get()` | O(1) | Direct array access |
| `set()` | O(1) | Direct array access |
| `free()` | O(1) | Direct bitmap update |
| `clear()` | O(1) | Memset operations |
| `len()` | O(n) | Counts allocated cells |
| `iter()` | O(n) | Linear scan |

## Use Cases

### Embedded Systems
Perfect for memory-constrained environments where dynamic allocation is unavailable or undesirable:

```rust
#![no_std]
use pwn_arena::Arena;

static NODES: Arena<Node, 256> = Arena::new(Node::default());
```

### Game Development
Manage game entities with predictable memory usage:

```rust
struct Entity {
    position: [f32; 3],
    velocity: [f32; 3],
    health: i32,
}

let entities: Arena<Entity, 10000> = Arena::new(Entity::default());
```

### Parsers and Compilers
Build AST nodes without heap allocation:

```rust
enum AstNode {
    Literal(i32),
    BinaryOp { op: char, left: ArenaIndex, right: ArenaIndex },
}

let ast: Arena<AstNode, 4096> = Arena::new(AstNode::Literal(0));
```

### Data Structures
Implement linked lists, trees, graphs:

```rust
struct ListNode {
    value: i32,
    next: Option<ArenaIndex>,
}

let list: Arena<ListNode, 100> = Arena::new(ListNode { value: 0, next: None });
```

## Limitations

- **Fixed capacity**: Size must be known at compile time
- **Copy types only**: `T` must implement `Copy` trait
- **No destructors**: Freed cells don't run drop logic
- **Interior mutability overhead**: Uses `RefCell` for safe access
- **Linear search**: Allocation may be O(n) in fragmented state

## Comparison with Alternatives

| Feature | pwn_arena | typed-arena | bumpalo |
|---------|-----------|-------------|---------|
| no_std support | ✅ | ❌ | ❌ |
| Fixed size | ✅ | ❌ | ❌ |
| Free individual items | ✅ | ❌ | ❌ |
| Zero dependencies | ✅ | ❌ | ❌ |
| Type-safe indices | ✅ | ❌ | ❌ |
| Recursive operations | ✅ | ❌ | ❌ |

## Safety

pwn_arena is safe Rust with no `unsafe` blocks. All operations are bounds-checked and type-safe. The `RefCell` ensures interior mutability safety at runtime.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Acknowledgments

Inspired by arena allocation patterns in game engines and compiler implementations.

---

**Note**: This is a specialized allocator designed for specific use cases. For general-purpose heap allocation, use the standard `Vec`, `Box`, or other allocators from the Rust standard library.
