// tests/arena_tests.rs

use pwn_arena::{Arena, ArenaCopy, ArenaDelete, ArenaError, ArenaIndex, ArenaResult};

// ============================================================================
// Basic Functionality Tests
// ============================================================================

#[test]
fn test_new_arena_is_empty() {
    let arena: Arena<i32, 10> = Arena::new(0);
    assert_eq!(arena.len(), 0);
    assert!(arena.is_empty());
    assert!(!arena.is_full());
    assert_eq!(arena.capacity(), 10);
    assert_eq!(arena.available(), 10);
}

#[test]
fn test_basic_allocation() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx1 = arena.alloc(42).unwrap();
    let idx2 = arena.alloc(43).unwrap();
    let idx3 = arena.alloc(44).unwrap();

    assert_eq!(arena.get(idx1).unwrap(), 42);
    assert_eq!(arena.get(idx2).unwrap(), 43);
    assert_eq!(arena.get(idx3).unwrap(), 44);
    assert_eq!(arena.len(), 3);
    assert_eq!(arena.available(), 7);
}

#[test]
fn test_alloc_returns_different_indices() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx1 = arena.alloc(1).unwrap();
    let idx2 = arena.alloc(2).unwrap();
    let idx3 = arena.alloc(3).unwrap();

    assert_ne!(idx1, idx2);
    assert_ne!(idx2, idx3);
    assert_ne!(idx1, idx3);
}

#[test]
fn test_out_of_memory() {
    let arena: Arena<i32, 3> = Arena::new(0);

    assert!(arena.alloc(1).is_ok());
    assert!(arena.alloc(2).is_ok());
    assert!(arena.alloc(3).is_ok());

    assert_eq!(arena.alloc(4), Err(ArenaError::OutOfMemory));
    assert!(arena.is_full());
    assert_eq!(arena.available(), 0);
}

#[test]
fn test_get_invalid_index() {
    let arena: Arena<i32, 10> = Arena::new(0);

    // Out of bounds index with any generation
    let invalid_idx = ArenaIndex::new(100, 0);
    assert_eq!(arena.get(invalid_idx), Err(ArenaError::InvalidIndex));
}

#[test]
fn test_get_freed_index() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx = arena.alloc(42).unwrap();
    arena.free(idx).unwrap();

    // After freeing, the generation increments, so we get GenerationMismatch
    assert_eq!(arena.get(idx), Err(ArenaError::GenerationMismatch));
}

// ============================================================================
// Free and Reuse Tests
// ============================================================================

#[test]
fn test_free_and_reuse() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx1 = arena.alloc(42).unwrap();
    assert_eq!(arena.len(), 1);

    arena.free(idx1).unwrap();
    assert_eq!(arena.len(), 0);
    assert!(arena.is_empty());

    let idx2 = arena.alloc(43).unwrap();
    assert_eq!(arena.len(), 1);
    assert_eq!(arena.get(idx2).unwrap(), 43);
}

#[test]
fn test_free_invalid_index() {
    let arena: Arena<i32, 10> = Arena::new(0);

    // Out of bounds index
    let invalid_idx = ArenaIndex::new(100, 0);
    assert_eq!(arena.free(invalid_idx), Err(ArenaError::InvalidIndex));
}

#[test]
fn test_double_free() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx = arena.alloc(42).unwrap();
    arena.free(idx).unwrap();

    // Double free returns GenerationMismatch because generation was incremented
    assert_eq!(arena.free(idx), Err(ArenaError::GenerationMismatch));
}

#[test]
fn test_fragmentation_and_reuse() {
    let arena: Arena<i32, 10> = Arena::new(0);

    // Allocate 5 items
    let idx1 = arena.alloc(1).unwrap();
    let idx2 = arena.alloc(2).unwrap();
    let idx3 = arena.alloc(3).unwrap();
    let idx4 = arena.alloc(4).unwrap();
    let idx5 = arena.alloc(5).unwrap();

    // Free every other one
    arena.free(idx2).unwrap();
    arena.free(idx4).unwrap();

    assert_eq!(arena.len(), 3);
    assert_eq!(arena.available(), 7);

    // Should be able to reuse freed slots
    let idx6 = arena.alloc(6).unwrap();
    let idx7 = arena.alloc(7).unwrap();

    assert_eq!(arena.len(), 5);
    assert_eq!(arena.get(idx6).unwrap(), 6);
    assert_eq!(arena.get(idx7).unwrap(), 7);
}

// ============================================================================
// Set Tests
// ============================================================================

#[test]
fn test_set_value() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx = arena.alloc(42).unwrap();
    assert_eq!(arena.get(idx).unwrap(), 42);

    arena.set(idx, 100).unwrap();
    assert_eq!(arena.get(idx).unwrap(), 100);

    arena.set(idx, -50).unwrap();
    assert_eq!(arena.get(idx).unwrap(), -50);
}

#[test]
fn test_set_invalid_index() {
    let arena: Arena<i32, 10> = Arena::new(0);

    // Out of bounds index
    let invalid_idx = ArenaIndex::new(100, 0);
    assert_eq!(arena.set(invalid_idx, 42), Err(ArenaError::InvalidIndex));
}

#[test]
fn test_set_freed_index() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx = arena.alloc(42).unwrap();
    arena.free(idx).unwrap();

    // After freeing, the generation increments
    assert_eq!(arena.set(idx, 100), Err(ArenaError::GenerationMismatch));
}

// ============================================================================
// Clear Tests
// ============================================================================

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
    assert_eq!(arena.available(), 10);
}

#[test]
fn test_clear_allows_full_reuse() {
    let arena: Arena<i32, 5> = Arena::new(0);

    // Fill the arena
    for i in 0..5 {
        arena.alloc(i).unwrap();
    }
    assert!(arena.is_full());

    arena.clear();

    // Should be able to allocate again
    for i in 0..5 {
        assert!(arena.alloc(i * 10).is_ok());
    }
    assert!(arena.is_full());
}

// ============================================================================
// Iterator Tests
// ============================================================================

#[test]
fn test_iter_empty() {
    let arena: Arena<i32, 10> = Arena::new(0);
    let count = arena.iter().count();
    assert_eq!(count, 0);
}

#[test]
fn test_iter_values() {
    let arena: Arena<i32, 10> = Arena::new(0);

    arena.alloc(1).unwrap();
    arena.alloc(2).unwrap();
    arena.alloc(3).unwrap();

    let values: Vec<i32> = arena.iter().map(|(_, v)| v).collect();
    assert_eq!(values.len(), 3);
    assert!(values.contains(&1));
    assert!(values.contains(&2));
    assert!(values.contains(&3));
}

#[test]
fn test_iter_indices() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx1 = arena.alloc(10).unwrap();
    let idx2 = arena.alloc(20).unwrap();
    let idx3 = arena.alloc(30).unwrap();

    let indices: Vec<ArenaIndex> = arena.iter().map(|(i, _)| i).collect();
    assert_eq!(indices.len(), 3);
    assert!(indices.contains(&idx1));
    assert!(indices.contains(&idx2));
    assert!(indices.contains(&idx3));
}

#[test]
fn test_iter_with_gaps() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx1 = arena.alloc(1).unwrap();
    let idx2 = arena.alloc(2).unwrap();
    let idx3 = arena.alloc(3).unwrap();
    let idx4 = arena.alloc(4).unwrap();

    // Free middle items
    arena.free(idx2).unwrap();
    arena.free(idx3).unwrap();

    let values: Vec<i32> = arena.iter().map(|(_, v)| v).collect();
    assert_eq!(values.len(), 2);
    assert!(values.contains(&1));
    assert!(values.contains(&4));
    assert!(!values.contains(&2));
    assert!(!values.contains(&3));
}

// ============================================================================
// Statistics Tests
// ============================================================================

#[test]
fn test_stats_empty() {
    let arena: Arena<i32, 10> = Arena::new(0);
    let stats = arena.stats();

    assert_eq!(stats.capacity, 10);
    assert_eq!(stats.allocated, 0);
    assert_eq!(stats.free, 10);
    assert_eq!(stats.usage_percent(), 0.0);
}

#[test]
fn test_stats_partial() {
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
fn test_stats_full() {
    let arena: Arena<i32, 5> = Arena::new(0);

    for i in 0..5 {
        arena.alloc(i).unwrap();
    }

    let stats = arena.stats();
    assert_eq!(stats.capacity, 5);
    assert_eq!(stats.allocated, 5);
    assert_eq!(stats.free, 0);
    assert_eq!(stats.usage_percent(), 100.0);
}

// ============================================================================
// is_allocated Tests
// ============================================================================

#[test]
fn test_is_allocated() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx = arena.alloc(42).unwrap();
    assert!(arena.is_allocated(idx));

    arena.free(idx).unwrap();
    assert!(!arena.is_allocated(idx));
}

#[test]
fn test_is_allocated_invalid_index() {
    let arena: Arena<i32, 10> = Arena::new(0);

    // Out of bounds index
    let invalid_idx = ArenaIndex::new(100, 0);
    assert!(!arena.is_allocated(invalid_idx));
}

// ============================================================================
// Type Tests (different types)
// ============================================================================

#[test]
fn test_arena_with_floats() {
    let arena: Arena<f64, 10> = Arena::new(0.0);

    let idx1 = arena.alloc(3.14).unwrap();
    let idx2 = arena.alloc(2.71828).unwrap();

    assert_eq!(arena.get(idx1).unwrap(), 3.14);
    assert_eq!(arena.get(idx2).unwrap(), 2.71828);
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Point {
    x: i32,
    y: i32,
}

#[test]
fn test_arena_with_struct() {
    let arena: Arena<Point, 10> = Arena::new(Point { x: 0, y: 0 });

    let p1 = Point { x: 10, y: 20 };
    let p2 = Point { x: 30, y: 40 };

    let idx1 = arena.alloc(p1).unwrap();
    let idx2 = arena.alloc(p2).unwrap();

    assert_eq!(arena.get(idx1).unwrap(), p1);
    assert_eq!(arena.get(idx2).unwrap(), p2);
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Color {
    Red,
    Green,
    Blue,
    RGB(u8, u8, u8),
}

#[test]
fn test_arena_with_enum() {
    let arena: Arena<Color, 10> = Arena::new(Color::Red);

    let idx1 = arena.alloc(Color::Green).unwrap();
    let idx2 = arena.alloc(Color::RGB(128, 255, 64)).unwrap();

    assert_eq!(arena.get(idx1).unwrap(), Color::Green);
    assert_eq!(arena.get(idx2).unwrap(), Color::RGB(128, 255, 64));
}

// ============================================================================
// Recursive Tree Tests
// ============================================================================

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
fn test_simple_tree() {
    let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

    let left = arena.alloc(Tree::Leaf(1)).unwrap();
    let right = arena.alloc(Tree::Leaf(2)).unwrap();
    let root = arena.alloc(Tree::Branch(left, right)).unwrap();

    match arena.get(root).unwrap() {
        Tree::Branch(l, r) => {
            assert_eq!(arena.get(l).unwrap(), Tree::Leaf(1));
            assert_eq!(arena.get(r).unwrap(), Tree::Leaf(2));
        }
        _ => panic!("Expected branch"),
    }
}

#[test]
fn test_recursive_delete_leaf() {
    let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

    let leaf = arena.alloc(Tree::Leaf(42)).unwrap();
    assert_eq!(arena.len(), 1);

    arena.delete_recursive(leaf).unwrap();
    assert_eq!(arena.len(), 0);
}

#[test]
fn test_recursive_delete_simple_tree() {
    let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

    let left = arena.alloc(Tree::Leaf(1)).unwrap();
    let right = arena.alloc(Tree::Leaf(2)).unwrap();
    let root = arena.alloc(Tree::Branch(left, right)).unwrap();

    assert_eq!(arena.len(), 3);

    arena.delete_recursive(root).unwrap();

    assert_eq!(arena.len(), 0);
}

#[test]
fn test_recursive_delete_complex_tree() {
    let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

    // Build tree: ((1, 2), (3, 4))
    let leaf1 = arena.alloc(Tree::Leaf(1)).unwrap();
    let leaf2 = arena.alloc(Tree::Leaf(2)).unwrap();
    let left_branch = arena.alloc(Tree::Branch(leaf1, leaf2)).unwrap();

    let leaf3 = arena.alloc(Tree::Leaf(3)).unwrap();
    let leaf4 = arena.alloc(Tree::Leaf(4)).unwrap();
    let right_branch = arena.alloc(Tree::Branch(leaf3, leaf4)).unwrap();

    let root = arena
        .alloc(Tree::Branch(left_branch, right_branch))
        .unwrap();

    assert_eq!(arena.len(), 7);

    arena.delete_recursive(root).unwrap();

    assert_eq!(arena.len(), 0);
}

#[test]
fn test_deep_copy_leaf() {
    let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

    let leaf = arena.alloc(Tree::Leaf(42)).unwrap();
    let copied = arena.copy_deep(leaf).unwrap();

    assert_eq!(arena.len(), 2);
    assert_eq!(arena.get(copied).unwrap(), Tree::Leaf(42));
    assert_ne!(leaf, copied);
}

#[test]
fn test_deep_copy_simple_tree() {
    let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

    let left = arena.alloc(Tree::Leaf(1)).unwrap();
    let right = arena.alloc(Tree::Leaf(2)).unwrap();
    let root = arena.alloc(Tree::Branch(left, right)).unwrap();

    let copied_root = arena.copy_deep(root).unwrap();

    // Should have 6 nodes total (3 original + 3 copied)
    assert_eq!(arena.len(), 6);

    // Verify structure is copied
    match arena.get(copied_root).unwrap() {
        Tree::Branch(cl, cr) => {
            assert_eq!(arena.get(cl).unwrap(), Tree::Leaf(1));
            assert_eq!(arena.get(cr).unwrap(), Tree::Leaf(2));
            // Indices should be different
            assert_ne!(cl, left);
            assert_ne!(cr, right);
        }
        _ => panic!("Expected branch"),
    }
}

#[test]
fn test_deep_copy_complex_tree() {
    let arena: Arena<Tree, 100> = Arena::new(Tree::Leaf(0));

    // Build tree: ((1, 2), (3, 4))
    let leaf1 = arena.alloc(Tree::Leaf(1)).unwrap();
    let leaf2 = arena.alloc(Tree::Leaf(2)).unwrap();
    let left_branch = arena.alloc(Tree::Branch(leaf1, leaf2)).unwrap();

    let leaf3 = arena.alloc(Tree::Leaf(3)).unwrap();
    let leaf4 = arena.alloc(Tree::Leaf(4)).unwrap();
    let right_branch = arena.alloc(Tree::Branch(leaf3, leaf4)).unwrap();

    let root = arena
        .alloc(Tree::Branch(left_branch, right_branch))
        .unwrap();

    assert_eq!(arena.len(), 7);

    let copied_root = arena.copy_deep(root).unwrap();

    // Should have 14 nodes total (7 original + 7 copied)
    assert_eq!(arena.len(), 14);

    // Verify copied tree has same values
    match arena.get(copied_root).unwrap() {
        Tree::Branch(cl_branch, cr_branch) => {
            match arena.get(cl_branch).unwrap() {
                Tree::Branch(cl1, cl2) => {
                    assert_eq!(arena.get(cl1).unwrap(), Tree::Leaf(1));
                    assert_eq!(arena.get(cl2).unwrap(), Tree::Leaf(2));
                }
                _ => panic!("Expected branch"),
            }
            match arena.get(cr_branch).unwrap() {
                Tree::Branch(cr3, cr4) => {
                    assert_eq!(arena.get(cr3).unwrap(), Tree::Leaf(3));
                    assert_eq!(arena.get(cr4).unwrap(), Tree::Leaf(4));
                }
                _ => panic!("Expected branch"),
            }
        }
        _ => panic!("Expected branch"),
    }
}

// ============================================================================
// Edge Cases and Stress Tests
// ============================================================================

#[test]
fn test_single_cell_arena() {
    let arena: Arena<i32, 1> = Arena::new(0);

    let idx = arena.alloc(42).unwrap();
    assert!(arena.is_full());
    assert_eq!(arena.alloc(43), Err(ArenaError::OutOfMemory));

    assert_eq!(arena.get(idx).unwrap(), 42);

    arena.free(idx).unwrap();
    assert!(arena.is_empty());

    let idx2 = arena.alloc(100).unwrap();
    assert_eq!(arena.get(idx2).unwrap(), 100);
}

#[test]
fn test_alternating_alloc_free() {
    let arena: Arena<i32, 10> = Arena::new(0);

    for i in 0..100 {
        let idx = arena.alloc(i).unwrap();
        assert_eq!(arena.len(), 1);
        assert_eq!(arena.get(idx).unwrap(), i);
        arena.free(idx).unwrap();
        assert_eq!(arena.len(), 0);
    }
}

#[test]
fn test_fill_and_empty_repeatedly() {
    let arena: Arena<i32, 5> = Arena::new(0);

    for round in 0..10 {
        let mut indices = Vec::new();

        // Fill arena
        for i in 0..5 {
            let idx = arena.alloc(round * 10 + i).unwrap();
            indices.push(idx);
        }
        assert!(arena.is_full());

        // Empty arena
        for idx in indices {
            arena.free(idx).unwrap();
        }
        assert!(arena.is_empty());
    }
}

#[test]
fn test_index_consistency() {
    let arena: Arena<i32, 10> = Arena::new(0);

    let idx1 = arena.alloc(100).unwrap();
    let idx2 = arena.alloc(200).unwrap();
    let idx3 = arena.alloc(300).unwrap();

    // Indices should remain valid and consistent
    assert_eq!(arena.get(idx1).unwrap(), 100);
    assert_eq!(arena.get(idx2).unwrap(), 200);
    assert_eq!(arena.get(idx3).unwrap(), 300);

    // Even after modifying other values
    arena.set(idx2, 250).unwrap();

    assert_eq!(arena.get(idx1).unwrap(), 100);
    assert_eq!(arena.get(idx2).unwrap(), 250);
    assert_eq!(arena.get(idx3).unwrap(), 300);
}

#[test]
fn test_arena_index_api() {
    let idx = ArenaIndex::new(42, 5);
    assert_eq!(idx.raw(), 42);
    assert_eq!(idx.generation(), 5);

    // Test that indices with same slot but different generations are not equal
    let idx2 = ArenaIndex::new(42, 6);
    assert_ne!(idx, idx2);
    assert_eq!(idx.raw(), idx2.raw());

    // Test that indices with same generation but different slots are not equal
    let idx3 = ArenaIndex::new(43, 5);
    assert_ne!(idx, idx3);
}

#[test]
fn test_large_arena() {
    let arena: Arena<i32, 1000> = Arena::new(0);

    // Allocate many items
    let mut indices = Vec::new();
    for i in 0..500 {
        let idx = arena.alloc(i).unwrap();
        indices.push(idx);
    }

    assert_eq!(arena.len(), 500);
    assert_eq!(arena.available(), 500);

    // Verify all values
    for (i, &idx) in indices.iter().enumerate() {
        assert_eq!(arena.get(idx).unwrap(), i as i32);
    }

    // Free half
    for &idx in indices.iter().take(250) {
        arena.free(idx).unwrap();
    }

    assert_eq!(arena.len(), 250);
    assert_eq!(arena.available(), 750);
}
