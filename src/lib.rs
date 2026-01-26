#![no_std]

//! # Fixed-Size Arena Allocator
//!
//! A minimal no-std, no-alloc arena allocator with fixed capacity.
//!
//! ## Features
//!
//! - **Fixed-size**: All memory pre-allocated at compile time
//! - **No-std, no-alloc**: Works in embedded environments
//! - **Generic**: Works with any `Copy` type
//! - **Interior mutability**: Safe concurrent access via `RefCell`
//! - **Zero dependencies**: Only uses `RefCell`
//!
//! ## Example
//!
//! ```rust
//! use pwn_arena::{Arena, ArenaIndex};
//!
//! #[derive(Clone, Copy, Debug, PartialEq)]
//! enum Node {
//!     Leaf(i32),
//!     Branch(ArenaIndex, ArenaIndex),
//! }
//!
//! let arena: Arena<Node, 1024> = Arena::new(Node::Leaf(0));
//!
//! // Allocate nodes
//! let left = arena.alloc(Node::Leaf(1)).unwrap();
//! let right = arena.alloc(Node::Leaf(2)).unwrap();
//! let root = arena.alloc(Node::Branch(left, right)).unwrap();
//!
//! // Access nodes
//! if let Node::Branch(l, r) = arena.get(root).unwrap() {
//!     println!("Left: {:?}, Right: {:?}", arena.get(l), arena.get(r));
//! }
//!
//! // Free when done
//! arena.free(root).unwrap();
//! ```

use core::cell::RefCell;

// ============================================================================
// Types
// ============================================================================

/// Index into the arena.
///
/// This is a type-safe wrapper around `usize` to prevent mixing indices
/// from different arenas or using raw integers accidentally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArenaIndex(usize);

impl ArenaIndex {
    /// Create a new arena index.
    #[inline]
    pub const fn new(index: usize) -> Self {
        ArenaIndex(index)
    }

    /// Get the raw index value.
    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }
}

impl From<usize> for ArenaIndex {
    fn from(index: usize) -> Self {
        ArenaIndex(index)
    }
}

impl From<ArenaIndex> for usize {
    fn from(index: ArenaIndex) -> Self {
        index.0
    }
}

/// Errors that can occur during arena operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaError {
    /// Arena is full, cannot allocate more cells.
    OutOfMemory,

    /// Invalid index (out of bounds or not allocated).
    InvalidIndex,
}

/// Result type for arena operations.
pub type ArenaResult<T> = Result<T, ArenaError>;

// ============================================================================
// Arena Implementation
// ============================================================================

/// Fixed-size arena allocator.
///
/// # Type Parameters
///
/// - `T`: The type of values stored (must be `Copy` for array initialization)
/// - `N`: Maximum number of cells (const generic)
///
/// # Memory Layout
///
/// - `cells`: Array of `T` values
/// - `allocated`: Bitmap tracking which cells are in use
/// - `next_free`: Hint for next free cell (optimization)
///
/// # Size
///
/// For type `T` and capacity `N`: `N * sizeof(T) + N * 1 + 8` bytes
pub struct Arena<T: Copy, const N: usize> {
    cells: RefCell<[T; N]>,
    allocated: RefCell<[bool; N]>,
    next_free: RefCell<usize>,
}

impl<T: Copy, const N: usize> Arena<T, N> {
    /// Create a new arena.
    ///
    /// All cells are initialized to `default_value`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use pwn_arena::Arena;
    ///
    /// let arena: Arena<i32, 100> = Arena::new(0);
    /// ```
    pub const fn new(default_value: T) -> Self {
        Arena {
            cells: RefCell::new([default_value; N]),
            allocated: RefCell::new([false; N]),
            next_free: RefCell::new(0),
        }
    }

    /// Get the maximum capacity of this arena.
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Get the number of currently allocated cells.
    pub fn len(&self) -> usize {
        self.allocated.borrow().iter().filter(|&&x| x).count()
    }

    /// Check if the arena is empty (no allocated cells).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if the arena is full (all cells allocated).
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == N
    }

    /// Get the number of free cells.
    #[inline]
    pub fn available(&self) -> usize {
        N - self.len()
    }

    /// Allocate a new cell and return its index.
    ///
    /// Uses a simple first-fit strategy with a hint for the next free cell.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::OutOfMemory` if the arena is full.
    ///
    /// # Example
    ///
    /// ```rust
    /// use pwn_arena::Arena;
    ///
    /// let arena: Arena<i32, 10> = Arena::new(0);
    /// let idx = arena.alloc(42).unwrap();
    /// assert_eq!(arena.get(idx).unwrap(), 42);
    /// ```
    pub fn alloc(&self, value: T) -> ArenaResult<ArenaIndex> {
        let mut allocated = self.allocated.borrow_mut();
        let mut cells = self.cells.borrow_mut();
        let mut next_free = self.next_free.borrow_mut();

        // Start search from hint
        for i in 0..N {
            let idx = (*next_free + i) % N;
            if !allocated[idx] {
                cells[idx] = value;
                allocated[idx] = true;

                // Update hint to next position
                *next_free = (idx + 1) % N;

                return Ok(ArenaIndex::new(idx));
            }
        }

        Err(ArenaError::OutOfMemory)
    }

    /// Get a copy of the value at the given index.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidIndex` if the index is out of bounds or not allocated.
    pub fn get(&self, index: ArenaIndex) -> ArenaResult<T> {
        let idx = index.raw();

        if idx >= N {
            return Err(ArenaError::InvalidIndex);
        }

        let allocated = self.allocated.borrow();
        if !allocated[idx] {
            return Err(ArenaError::InvalidIndex);
        }

        Ok(self.cells.borrow()[idx])
    }

    /// Set the value at the given index.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidIndex` if the index is out of bounds or not allocated.
    pub fn set(&self, index: ArenaIndex, value: T) -> ArenaResult<()> {
        let idx = index.raw();

        if idx >= N {
            return Err(ArenaError::InvalidIndex);
        }

        let allocated = self.allocated.borrow();
        if !allocated[idx] {
            return Err(ArenaError::InvalidIndex);
        }
        drop(allocated);

        self.cells.borrow_mut()[idx] = value;
        Ok(())
    }

    /// Free a cell, making it available for reuse.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidIndex` if the index is out of bounds or not allocated.
    ///
    /// # Example
    ///
    /// ```rust
    /// use pwn_arena::Arena;
    ///
    /// let arena: Arena<i32, 10> = Arena::new(0);
    /// let idx = arena.alloc(42).unwrap();
    /// arena.free(idx).unwrap();
    /// assert_eq!(arena.len(), 0);
    /// ```
    pub fn free(&self, index: ArenaIndex) -> ArenaResult<()> {
        let idx = index.raw();

        if idx >= N {
            return Err(ArenaError::InvalidIndex);
        }

        let mut allocated = self.allocated.borrow_mut();
        if !allocated[idx] {
            return Err(ArenaError::InvalidIndex);
        }

        allocated[idx] = false;
        Ok(())
    }

    /// Check if an index is currently allocated.
    #[inline]
    pub fn is_allocated(&self, index: ArenaIndex) -> bool {
        let idx = index.raw();
        idx < N && self.allocated.borrow()[idx]
    }

    /// Clear all allocations, making the entire arena available.
    ///
    /// # Warning
    ///
    /// This does not call any destructors. Use with caution.
    pub fn clear(&self) {
        *self.allocated.borrow_mut() = [false; N];
        *self.next_free.borrow_mut() = 0;
    }

    /// Iterate over all allocated indices and values.
    ///
    /// # Example
    ///
    /// ```rust
    /// use pwn_arena::Arena;
    ///
    /// let arena: Arena<i32, 10> = Arena::new(0);
    /// arena.alloc(1).unwrap();
    /// arena.alloc(2).unwrap();
    /// arena.alloc(3).unwrap();
    ///
    /// for (idx, value) in arena.iter() {
    ///     println!("Index {:?}: {}", idx, value);
    /// }
    /// ```
    pub fn iter(&self) -> ArenaIterator<'_, T, N> {
        ArenaIterator {
            arena: self,
            current: 0,
        }
    }

    /// Get statistics about arena usage.
    pub fn stats(&self) -> ArenaStats {
        let allocated = self.len();
        ArenaStats {
            capacity: N,
            allocated,
            free: N - allocated,
            fragmentation: self.calculate_fragmentation(),
        }
    }

    fn calculate_fragmentation(&self) -> f32 {
        let allocated = self.allocated.borrow();
        let mut fragments = 0;
        let mut in_free = false;

        for &is_allocated in allocated.iter() {
            if !is_allocated {
                if !in_free {
                    fragments += 1;
                    in_free = true;
                }
            } else {
                in_free = false;
            }
        }

        if fragments == 0 {
            0.0
        } else {
            fragments as f32 / N as f32
        }
    }
}

// ============================================================================
// Recursive Deletion Support
// ============================================================================

/// Trait for types that can be recursively deleted from the arena.
///
/// Implement this for types that contain `ArenaIndex` fields pointing
/// to other allocations that should be freed together.
///
/// # Example
///
/// ```rust
/// use pwn_arena::{Arena, ArenaIndex, ArenaDelete, ArenaResult};
///
/// #[derive(Clone, Copy)]
/// enum Tree {
///     Leaf(i32),
///     Branch(ArenaIndex, ArenaIndex),
/// }
///
/// impl ArenaDelete<Tree, 100> for Tree {
///     fn delete_recursive(&self, arena: &Arena<Tree, 100>) -> ArenaResult<()> {
///         match *self {
///             Tree::Leaf(_) => Ok(()),
///             Tree::Branch(left, right) => {
///                 arena.delete_recursive(left)?;
///                 arena.delete_recursive(right)?;
///                 Ok(())
///             }
///         }
///     }
/// }
/// ```
pub trait ArenaDelete<T: Copy, const N: usize> {
    /// Recursively delete this value and any children from the arena.
    fn delete_recursive(&self, arena: &Arena<T, N>) -> ArenaResult<()>;
}

impl<T: Copy, const N: usize> Arena<T, N> {
    /// Delete a value and recursively delete any children.
    ///
    /// This requires `T: ArenaDelete<T, N>`.
    pub fn delete_recursive(&self, index: ArenaIndex) -> ArenaResult<()>
    where
        T: ArenaDelete<T, N>,
    {
        let value = self.get(index)?;
        value.delete_recursive(self)?;
        self.free(index)?;
        Ok(())
    }
}

// ============================================================================
// Copy Support
// ============================================================================

/// Trait for types that can be deep-copied within the arena.
///
/// Implement this for types containing `ArenaIndex` fields that need
/// to recursively copy their children.
///
/// # Example
///
/// ```rust
/// use pwn_arena::{Arena, ArenaIndex, ArenaCopy, ArenaResult};
///
/// #[derive(Clone, Copy)]
/// enum Tree {
///     Leaf(i32),
///     Branch(ArenaIndex, ArenaIndex),
/// }
///
/// impl ArenaCopy<Tree, 100> for Tree {
///     fn copy_deep(&self, arena: &Arena<Tree, 100>) -> ArenaResult<Tree> {
///         match *self {
///             Tree::Leaf(n) => Ok(Tree::Leaf(n)),
///             Tree::Branch(left, right) => {
///                 let new_left = arena.copy_deep(left)?;
///                 let new_right = arena.copy_deep(right)?;
///                 Ok(Tree::Branch(new_left, new_right))
///             }
///         }
///     }
/// }
/// ```
pub trait ArenaCopy<T: Copy, const N: usize> {
    /// Create a deep copy of this value in the arena.
    fn copy_deep(&self, arena: &Arena<T, N>) -> ArenaResult<T>;
}

impl<T: Copy, const N: usize> Arena<T, N> {
    /// Create a deep copy of a value and its children.
    ///
    /// This requires `T: ArenaCopy<T, N>`.
    pub fn copy_deep(&self, index: ArenaIndex) -> ArenaResult<ArenaIndex>
    where
        T: ArenaCopy<T, N>,
    {
        let value = self.get(index)?;
        let copied = value.copy_deep(self)?;
        self.alloc(copied)
    }
}

// ============================================================================
// Iterator
// ============================================================================

/// Iterator over allocated cells in the arena.
pub struct ArenaIterator<'a, T: Copy, const N: usize> {
    arena: &'a Arena<T, N>,
    current: usize,
}

impl<'a, T: Copy, const N: usize> Iterator for ArenaIterator<'a, T, N> {
    type Item = (ArenaIndex, T);

    fn next(&mut self) -> Option<Self::Item> {
        let allocated = self.arena.allocated.borrow();

        while self.current < N {
            let idx = self.current;
            self.current += 1;

            if allocated[idx] {
                let value = self.arena.cells.borrow()[idx];
                return Some((ArenaIndex::new(idx), value));
            }
        }

        None
    }
}

// ============================================================================
// Statistics
// ============================================================================

/// Statistics about arena usage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArenaStats {
    /// Total capacity of the arena.
    pub capacity: usize,

    /// Number of currently allocated cells.
    pub allocated: usize,

    /// Number of free cells.
    pub free: usize,

    /// Fragmentation ratio (0.0 = not fragmented, 1.0 = highly fragmented).
    pub fragmentation: f32,
}

impl ArenaStats {
    /// Get usage as a percentage (0-100).
    pub fn usage_percent(&self) -> f32 {
        if self.capacity == 0 {
            0.0
        } else {
            (self.allocated as f32 / self.capacity as f32) * 100.0
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_allocation() {
        let arena: Arena<i32, 10> = Arena::new(0);

        let idx1 = arena.alloc(42).unwrap();
        let idx2 = arena.alloc(43).unwrap();

        assert_eq!(arena.get(idx1).unwrap(), 42);
        assert_eq!(arena.get(idx2).unwrap(), 43);
        assert_eq!(arena.len(), 2);
    }

    #[test]
    fn test_free_and_reuse() {
        let arena: Arena<i32, 10> = Arena::new(0);

        let idx1 = arena.alloc(42).unwrap();
        assert_eq!(arena.len(), 1);

        arena.free(idx1).unwrap();
        assert_eq!(arena.len(), 0);

        let idx2 = arena.alloc(43).unwrap();
        assert_eq!(arena.len(), 1);
        assert_eq!(arena.get(idx2).unwrap(), 43);
    }

    #[test]
    fn test_out_of_memory() {
        let arena: Arena<i32, 3> = Arena::new(0);

        assert!(arena.alloc(1).is_ok());
        assert!(arena.alloc(2).is_ok());
        assert!(arena.alloc(3).is_ok());
        assert_eq!(arena.alloc(4), Err(ArenaError::OutOfMemory));
    }

    #[test]
    fn test_invalid_index() {
        let arena: Arena<i32, 10> = Arena::new(0);

        let idx = arena.alloc(42).unwrap();
        arena.free(idx).unwrap();

        assert_eq!(arena.get(idx), Err(ArenaError::InvalidIndex));
    }

    #[test]
    fn test_stats() {
        let arena: Arena<i32, 10> = Arena::new(0);

        arena.alloc(1).unwrap();
        arena.alloc(2).unwrap();

        let stats = arena.stats();
        assert_eq!(stats.capacity, 10);
        assert_eq!(stats.allocated, 2);
        assert_eq!(stats.free, 8);
        assert_eq!(stats.usage_percent(), 20.0);
    }

    #[test]
    fn test_clear() {
        let arena: Arena<i32, 10> = Arena::new(0);

        arena.alloc(1).unwrap();
        arena.alloc(2).unwrap();
        arena.alloc(3).unwrap();

        assert_eq!(arena.len(), 3);

        arena.clear();

        assert_eq!(arena.len(), 0);
        assert!(arena.is_empty());
    }

    // Example of recursive deletion
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Tree {
        Leaf(i32),
        Branch(ArenaIndex, ArenaIndex),
    }

    impl ArenaDelete<Tree, 100> for Tree {
        fn delete_recursive(&self, arena: &Arena<Tree, 100>) -> ArenaResult<()> {
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

    impl ArenaCopy<Tree, 100> for Tree {
        fn copy_deep(&self, arena: &Arena<Tree, 100>) -> ArenaResult<Tree> {
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

    #[test]
    fn test_recursive_delete() {
        let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

        let left = arena.alloc(Tree::Leaf(1)).unwrap();
        let right = arena.alloc(Tree::Leaf(2)).unwrap();
        let root = arena.alloc(Tree::Branch(left, right)).unwrap();

        assert_eq!(arena.len(), 3);

        arena.delete_recursive(root).unwrap();

        assert_eq!(arena.len(), 0);
    }

    #[test]
    fn test_deep_copy() {
        let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

        let left = arena.alloc(Tree::Leaf(1)).unwrap();
        let right = arena.alloc(Tree::Leaf(2)).unwrap();
        let root = arena.alloc(Tree::Branch(left, right)).unwrap();

        let copied_root = arena.copy_deep(root).unwrap();

        // Should have 6 nodes total (3 original + 3 copied)
        assert_eq!(arena.len(), 6);

        // Verify structure is copied
        if let Tree::Branch(cl, cr) = arena.get(copied_root).unwrap() {
            assert_eq!(arena.get(cl).unwrap(), Tree::Leaf(1));
            assert_eq!(arena.get(cr).unwrap(), Tree::Leaf(2));
        } else {
            panic!("Expected branch");
        }
    }

    #[test]
    fn test_set() {
        let arena: Arena<i32, 10> = Arena::new(0);

        let idx = arena.alloc(42).unwrap();
        assert_eq!(arena.get(idx).unwrap(), 42);

        arena.set(idx, 100).unwrap();
        assert_eq!(arena.get(idx).unwrap(), 100);
    }
}
