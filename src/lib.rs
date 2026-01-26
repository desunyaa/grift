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
//! - **Generational indices**: Detects use-after-free (ABA problem)
//! - **O(1) allocation**: Free-list based allocation and deallocation
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

/// Index into the arena with generational tracking.
///
/// This is a type-safe wrapper that stores both the slot index and the
/// generation at which it was allocated. This prevents the ABA problem
/// where a freed and reallocated slot could be accessed by a stale index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArenaIndex {
    index: usize,
    generation: u32,
}

impl ArenaIndex {
    /// Create a new arena index with the given slot index and generation.
    #[inline]
    pub const fn new(index: usize, generation: u32) -> Self {
        ArenaIndex { index, generation }
    }

    /// Get the raw slot index value.
    #[inline]
    pub const fn raw(self) -> usize {
        self.index
    }

    /// Get the generation this index was created with.
    #[inline]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// Errors that can occur during arena operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaError {
    /// Arena is full, cannot allocate more cells.
    OutOfMemory,

    /// Invalid index (out of bounds, not allocated, or stale generation).
    InvalidIndex,

    /// The index's generation doesn't match the slot's current generation.
    /// This indicates a use-after-free attempt (ABA problem).
    GenerationMismatch,
}

/// Result type for arena operations.
pub type ArenaResult<T> = Result<T, ArenaError>;

// ============================================================================
// Slot Implementation (Free-List Support)
// ============================================================================

/// Sentinel value indicating end of free list.
const FREE_LIST_END: usize = usize::MAX;

/// Internal slot representation for free-list based allocation.
///
/// Each slot is either free (storing the next free slot index) or
/// occupied (storing the actual value).
#[derive(Clone, Copy)]
enum Slot<T: Copy> {
    /// Free slot containing index of the next free slot (or FREE_LIST_END).
    Free { next_free: usize },
    /// Occupied slot containing the stored value.
    Occupied { value: T },
}

// ============================================================================
// Arena Implementation
// ============================================================================

/// Fixed-size arena allocator with generational indices and O(1) allocation.
///
/// # Type Parameters
///
/// - `T`: The type of values stored (must be `Copy` for array initialization)
/// - `N`: Maximum number of cells (const generic)
///
/// # Memory Layout
///
/// - `slots`: Array of `Slot<T>` (either free with next pointer, or occupied with value)
/// - `generations`: Array of generation counters for each slot
/// - `free_head`: Head of the free list
/// - `len`: Number of currently allocated slots
///
/// # Generational Indices
///
/// Each slot has a generation counter that increments when freed. An `ArenaIndex`
/// stores the generation it was created with. If generations don't match during
/// access, `InvalidIndex` is returned, preventing the ABA problem.
///
/// # O(1) Allocation
///
/// Uses a free-list for constant-time allocation and deallocation instead of
/// scanning a bitmap.
pub struct Arena<T: Copy, const N: usize> {
    slots: RefCell<[Slot<T>; N]>,
    generations: RefCell<[u32; N]>,
    free_head: RefCell<usize>,
    len: RefCell<usize>,
}

impl<T: Copy, const N: usize> Arena<T, N> {
    /// Create a new arena.
    ///
    /// All slots start as free, linked together in a free list.
    ///
    /// # Example
    ///
    /// ```rust
    /// use pwn_arena::Arena;
    ///
    /// let arena: Arena<i32, 100> = Arena::new(0);
    /// ```
    pub fn new(_default_value: T) -> Self {
        // Initialize all slots as free, linked together
        // Slot 0 -> 1 -> 2 -> ... -> N-1 -> FREE_LIST_END
        let mut slots = [Slot::Free { next_free: FREE_LIST_END }; N];
        for i in 0..N {
            slots[i] = Slot::Free {
                next_free: if i + 1 < N { i + 1 } else { FREE_LIST_END },
            };
        }

        Arena {
            slots: RefCell::new(slots),
            generations: RefCell::new([0u32; N]),
            free_head: RefCell::new(if N > 0 { 0 } else { FREE_LIST_END }),
            len: RefCell::new(0),
        }
    }

    /// Get the maximum capacity of this arena.
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Get the number of currently allocated cells.
    #[inline]
    pub fn len(&self) -> usize {
        *self.len.borrow()
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
    /// Uses O(1) free-list based allocation.
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
        let mut free_head = self.free_head.borrow_mut();

        // Check if there's a free slot
        if *free_head == FREE_LIST_END {
            return Err(ArenaError::OutOfMemory);
        }

        let idx = *free_head;
        let mut slots = self.slots.borrow_mut();

        // Pop from free list
        let next_free = match slots[idx] {
            Slot::Free { next_free } => next_free,
            Slot::Occupied { .. } => unreachable!("free_head pointed to occupied slot"),
        };

        // Mark as occupied
        slots[idx] = Slot::Occupied { value };
        *free_head = next_free;

        // Get current generation for this slot
        let generation = self.generations.borrow()[idx];

        // Increment allocated count
        *self.len.borrow_mut() += 1;

        Ok(ArenaIndex::new(idx, generation))
    }

    /// Validate an index and return the slot index if valid.
    #[inline]
    fn validate_index(&self, index: ArenaIndex) -> ArenaResult<usize> {
        let idx = index.raw();

        if idx >= N {
            return Err(ArenaError::InvalidIndex);
        }

        // Check generation
        let current_gen = self.generations.borrow()[idx];
        if index.generation() != current_gen {
            return Err(ArenaError::GenerationMismatch);
        }

        // Check if slot is occupied
        match self.slots.borrow()[idx] {
            Slot::Occupied { .. } => Ok(idx),
            Slot::Free { .. } => Err(ArenaError::InvalidIndex),
        }
    }

    /// Get a copy of the value at the given index.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidIndex` if the index is out of bounds or not allocated.
    /// Returns `ArenaError::GenerationMismatch` if the index is stale (slot was freed and reused).
    pub fn get(&self, index: ArenaIndex) -> ArenaResult<T> {
        let idx = self.validate_index(index)?;

        match self.slots.borrow()[idx] {
            Slot::Occupied { value } => Ok(value),
            Slot::Free { .. } => Err(ArenaError::InvalidIndex),
        }
    }

    /// Set the value at the given index.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidIndex` if the index is out of bounds or not allocated.
    /// Returns `ArenaError::GenerationMismatch` if the index is stale.
    pub fn set(&self, index: ArenaIndex, value: T) -> ArenaResult<()> {
        let idx = self.validate_index(index)?;

        self.slots.borrow_mut()[idx] = Slot::Occupied { value };
        Ok(())
    }

    /// Free a cell, making it available for reuse.
    ///
    /// This increments the slot's generation, invalidating any existing indices
    /// to this slot.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidIndex` if the index is out of bounds or not allocated.
    /// Returns `ArenaError::GenerationMismatch` if the index is stale.
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
        let idx = self.validate_index(index)?;

        // Increment generation to invalidate any existing indices
        {
            let mut generations = self.generations.borrow_mut();
            generations[idx] = generations[idx].wrapping_add(1);
        }

        // Push onto free list
        let mut free_head = self.free_head.borrow_mut();
        self.slots.borrow_mut()[idx] = Slot::Free { next_free: *free_head };
        *free_head = idx;

        // Decrement allocated count
        *self.len.borrow_mut() -= 1;

        Ok(())
    }

    /// Check if an index is currently valid (allocated with matching generation).
    #[inline]
    pub fn is_allocated(&self, index: ArenaIndex) -> bool {
        self.validate_index(index).is_ok()
    }

    /// Clear all allocations, making the entire arena available.
    ///
    /// This increments all generations to invalidate existing indices.
    ///
    /// # Warning
    ///
    /// This does not call any destructors. Use with caution.
    pub fn clear(&self) {
        // Rebuild free list
        let mut slots = self.slots.borrow_mut();
        for i in 0..N {
            slots[i] = Slot::Free {
                next_free: if i + 1 < N { i + 1 } else { FREE_LIST_END },
            };
        }

        // Increment all generations to invalidate existing indices
        let mut generations = self.generations.borrow_mut();
        for g in generations.iter_mut() {
            *g = g.wrapping_add(1);
        }

        *self.free_head.borrow_mut() = if N > 0 { 0 } else { FREE_LIST_END };
        *self.len.borrow_mut() = 0;
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
        let slots = self.slots.borrow();
        let mut fragments = 0;
        let mut in_free = false;

        for slot in slots.iter() {
            match slot {
                Slot::Free { .. } => {
                    if !in_free {
                        fragments += 1;
                        in_free = true;
                    }
                }
                Slot::Occupied { .. } => {
                    in_free = false;
                }
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
        let slots = self.arena.slots.borrow();
        let generations = self.arena.generations.borrow();

        while self.current < N {
            let idx = self.current;
            self.current += 1;

            if let Slot::Occupied { value } = slots[idx] {
                let generation = generations[idx];
                return Some((ArenaIndex::new(idx, generation), value));
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

        // After freeing, the generation increments, so we get GenerationMismatch
        assert_eq!(arena.get(idx), Err(ArenaError::GenerationMismatch));
    }

    #[test]
    fn test_generational_indices_aba_protection() {
        let arena: Arena<i32, 10> = Arena::new(0);

        // Allocate and free a slot
        let old_idx = arena.alloc(42).unwrap();
        arena.free(old_idx).unwrap();

        // Allocate a new value in the same slot
        let new_idx = arena.alloc(999).unwrap();

        // The old index should now be invalid (ABA problem prevented)
        assert_eq!(arena.get(old_idx), Err(ArenaError::GenerationMismatch));
        assert_eq!(arena.free(old_idx), Err(ArenaError::GenerationMismatch));

        // The new index should work fine
        assert_eq!(arena.get(new_idx).unwrap(), 999);

        // Verify they point to the same raw slot but different generations
        assert_eq!(old_idx.raw(), new_idx.raw());
        assert_ne!(old_idx.generation(), new_idx.generation());
    }

    #[test]
    fn test_free_list_o1_allocation() {
        let arena: Arena<i32, 5> = Arena::new(0);

        // Allocate all slots
        let idx0 = arena.alloc(0).unwrap();
        let idx1 = arena.alloc(1).unwrap();
        let idx2 = arena.alloc(2).unwrap();
        let idx3 = arena.alloc(3).unwrap();
        let idx4 = arena.alloc(4).unwrap();

        assert!(arena.is_full());

        // Free some slots in non-sequential order
        arena.free(idx2).unwrap();
        arena.free(idx0).unwrap();
        arena.free(idx4).unwrap();

        assert_eq!(arena.len(), 2);
        assert_eq!(arena.available(), 3);

        // Allocate again - should reuse freed slots (LIFO order from free list)
        let new1 = arena.alloc(100).unwrap();
        let new2 = arena.alloc(200).unwrap();
        let new3 = arena.alloc(300).unwrap();

        assert_eq!(arena.len(), 5);

        // Verify the new values are accessible
        assert_eq!(arena.get(new1).unwrap(), 100);
        assert_eq!(arena.get(new2).unwrap(), 200);
        assert_eq!(arena.get(new3).unwrap(), 300);

        // Original unfreed indices should still work
        assert_eq!(arena.get(idx1).unwrap(), 1);
        assert_eq!(arena.get(idx3).unwrap(), 3);
    }

    #[test]
    fn test_clear_invalidates_all_indices() {
        let arena: Arena<i32, 10> = Arena::new(0);

        let idx1 = arena.alloc(1).unwrap();
        let idx2 = arena.alloc(2).unwrap();
        let idx3 = arena.alloc(3).unwrap();

        arena.clear();

        // All old indices should be invalid
        assert_eq!(arena.get(idx1), Err(ArenaError::GenerationMismatch));
        assert_eq!(arena.get(idx2), Err(ArenaError::GenerationMismatch));
        assert_eq!(arena.get(idx3), Err(ArenaError::GenerationMismatch));

        // New allocations should work
        let new_idx = arena.alloc(42).unwrap();
        assert_eq!(arena.get(new_idx).unwrap(), 42);
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
