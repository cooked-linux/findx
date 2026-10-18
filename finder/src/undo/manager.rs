//! Bounded multi-level undo and redo manager maintaining the transaction history.

use std::collections::VecDeque;

use super::error::Result;
use super::inversion::{ConflictStrategy, execute_forward, execute_inverse};
use super::models::{FileOperationRecord, UndoTransaction};

/// Default maximum depth of the undo history ring buffer.
pub const DEFAULT_MAX_DEPTH: usize = 50;

/// Bounded multi-level undo/redo stack manager.
pub struct UndoManager {
    undo_stack: VecDeque<UndoTransaction>,
    redo_stack: VecDeque<UndoTransaction>,
    max_depth: usize,
    conflict_strategy: ConflictStrategy,
    listeners: Vec<Box<dyn Fn() + Send + Sync>>,
}

impl Default for UndoManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for UndoManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoManager")
            .field("undo_depth", &self.undo_stack.len())
            .field("redo_depth", &self.redo_stack.len())
            .field("max_depth", &self.max_depth)
            .field("conflict_strategy", &self.conflict_strategy)
            .finish_non_exhaustive()
    }
}

impl UndoManager {
    /// Constructs a new [`UndoManager`] with the default depth of 50.
    #[must_use]
    pub fn new() -> Self {
        Self::with_depth(DEFAULT_MAX_DEPTH)
    }

    /// Constructs a new [`UndoManager`] with a custom stack depth ceiling.
    #[must_use]
    pub fn with_depth(max_depth: usize) -> Self {
        Self {
            undo_stack: VecDeque::with_capacity(max_depth),
            redo_stack: VecDeque::new(),
            max_depth,
            conflict_strategy: ConflictStrategy::default(),
            listeners: Vec::new(),
        }
    }

    /// Reconstructs an [`UndoManager`] from pre-existing undo and redo stacks (e.g. from journal).
    #[must_use]
    pub fn from_stacks(
        undo_stack: VecDeque<UndoTransaction>,
        redo_stack: VecDeque<UndoTransaction>,
        max_depth: usize,
    ) -> Self {
        Self {
            undo_stack,
            redo_stack,
            max_depth,
            conflict_strategy: ConflictStrategy::default(),
            listeners: Vec::new(),
        }
    }

    /// Sets the collision strategy for subsequent undo/redo operations.
    pub const fn set_conflict_strategy(&mut self, strategy: ConflictStrategy) {
        self.conflict_strategy = strategy;
    }

    /// Returns the currently configured collision strategy.
    #[must_use]
    pub const fn conflict_strategy(&self) -> ConflictStrategy {
        self.conflict_strategy
    }

    /// Registers a change listener callback invoked whenever the undo or redo stack changes.
    pub fn add_listener<F>(&mut self, listener: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.listeners.push(Box::new(listener));
    }

    fn notify_listeners(&self) {
        for listener in &self.listeners {
            listener();
        }
    }

    /// Records a new completed operation transaction.
    ///
    /// Pushes the transaction onto the undo stack, trims oldest transactions if exceeding
    /// `max_depth`, clears the redo stack, and notifies listeners.
    pub fn record_transaction(&mut self, tx: UndoTransaction) {
        self.undo_stack.push_back(tx);
        while self.undo_stack.len() > self.max_depth {
            let _ = self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
        self.notify_listeners();
    }

    /// Executes the inverse of the most recent operation on the undo stack.
    ///
    /// # Errors
    /// Returns [`crate::undo::error::UndoError`] if inverse execution fails.
    pub fn undo(&mut self) -> Result<Option<UndoTransaction>> {
        self.undo_with_strategy(self.conflict_strategy)
    }

    /// Executes the inverse of the top undo transaction using an explicit collision strategy.
    ///
    /// # Errors
    /// Returns [`crate::undo::error::UndoError`] if inverse execution fails.
    pub fn undo_with_strategy(
        &mut self,
        strategy: ConflictStrategy,
    ) -> Result<Option<UndoTransaction>> {
        let Some(tx) = self.undo_stack.pop_back() else {
            return Ok(None);
        };

        if let Err(err) = execute_inverse(&tx, strategy) {
            // Restore transaction to undo stack on failure
            self.undo_stack.push_back(tx);
            return Err(err);
        }

        self.redo_stack.push_back(tx.clone());
        self.notify_listeners();
        Ok(Some(tx))
    }

    /// Re-executes the forward actions of the most recent undone transaction on the redo stack.
    ///
    /// # Errors
    /// Returns [`crate::undo::error::UndoError`] if forward re-execution fails.
    pub fn redo(&mut self) -> Result<Option<UndoTransaction>> {
        self.redo_with_strategy(self.conflict_strategy)
    }

    /// Re-executes the top redo transaction using an explicit collision strategy.
    ///
    /// # Errors
    /// Returns [`crate::undo::error::UndoError`] if forward re-execution fails.
    pub fn redo_with_strategy(
        &mut self,
        strategy: ConflictStrategy,
    ) -> Result<Option<UndoTransaction>> {
        let Some(tx) = self.redo_stack.pop_back() else {
            return Ok(None);
        };

        if let Err(err) = execute_forward(&tx, strategy) {
            // Restore transaction to redo stack on failure
            self.redo_stack.push_back(tx);
            return Err(err);
        }

        self.undo_stack.push_back(tx.clone());
        self.notify_listeners();
        Ok(Some(tx))
    }

    /// Returns `true` if an operation is available to be undone.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns `true` if an operation is available to be redone.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Returns the UI label for the current top undo transaction (e.g. `"Undo Move of 3 Items"`).
    #[must_use]
    pub fn current_undo_label(&self) -> Option<String> {
        self.undo_stack.back().map(UndoTransaction::undo_label)
    }

    /// Returns the UI label for the current top redo transaction (e.g. `"Redo Move of 3 Items"`).
    #[must_use]
    pub fn current_redo_label(&self) -> Option<String> {
        self.redo_stack.back().map(UndoTransaction::redo_label)
    }

    /// Read-only slice access to transactions on the undo stack.
    #[must_use]
    pub const fn undo_stack(&self) -> &VecDeque<UndoTransaction> {
        &self.undo_stack
    }

    /// Read-only slice access to transactions on the redo stack.
    #[must_use]
    pub const fn redo_stack(&self) -> &VecDeque<UndoTransaction> {
        &self.redo_stack
    }

    /// Maximum ring buffer depth.
    #[must_use]
    pub const fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Purges all transactions referencing file records that match the given predicate.
    ///
    /// Used when the trash is permanently emptied or files are irrecoverably deleted.
    pub fn purge_records_matching(&mut self, predicate: impl Fn(&FileOperationRecord) -> bool) {
        let prev_undo_len = self.undo_stack.len();
        let prev_redo_len = self.redo_stack.len();

        self.undo_stack
            .retain(|tx| !tx.records.iter().any(&predicate));
        self.redo_stack
            .retain(|tx| !tx.records.iter().any(&predicate));

        if self.undo_stack.len() != prev_undo_len || self.redo_stack.len() != prev_redo_len {
            self.notify_listeners();
        }
    }

    /// Clears all undo and redo history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.notify_listeners();
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::redundant_clone
)]
mod tests {
    use super::*;
    use crate::undo::models::UndoOperationType;
    use shared::fs::stat::stat_path;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::SystemTime;
    use tempfile::tempdir;

    #[test]
    fn test_undo_manager_sequence_and_clearing_redo() {
        let dir = tempdir().unwrap();
        let mut manager = UndoManager::new();

        let notifications = Arc::new(AtomicUsize::new(0));
        let notif_clone = Arc::clone(&notifications);
        manager.add_listener(move || {
            notif_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Create 3 operations
        let file1_src = dir.path().join("file1_src.txt");
        let file1_dst = dir.path().join("file1_dst.txt");
        fs::write(&file1_dst, "content1").unwrap();
        let stat1 = stat_path(&file1_dst).unwrap();
        let rec1 = FileOperationRecord::new(
            file1_src.clone(),
            file1_dst.clone(),
            stat1.ino,
            stat1.dev,
            SystemTime::now(),
            None,
        );
        let tx1 = UndoTransaction::new(
            "tx1",
            UndoOperationType::Move,
            "Move 1",
            vec![rec1],
            SystemTime::now(),
        );

        let file2_src = dir.path().join("file2_src.txt");
        let file2_dst = dir.path().join("file2_dst.txt");
        fs::write(&file2_dst, "content2").unwrap();
        let stat2 = stat_path(&file2_dst).unwrap();
        let rec2 = FileOperationRecord::new(
            file2_src.clone(),
            file2_dst.clone(),
            stat2.ino,
            stat2.dev,
            SystemTime::now(),
            None,
        );
        let tx2 = UndoTransaction::new(
            "tx2",
            UndoOperationType::Move,
            "Move 2",
            vec![rec2],
            SystemTime::now(),
        );

        let file3_src = dir.path().join("file3_src.txt");
        let file3_dst = dir.path().join("file3_dst.txt");
        fs::write(&file3_dst, "content3").unwrap();
        let stat3 = stat_path(&file3_dst).unwrap();
        let rec3 = FileOperationRecord::new(
            file3_src.clone(),
            file3_dst.clone(),
            stat3.ino,
            stat3.dev,
            SystemTime::now(),
            None,
        );
        let tx3 = UndoTransaction::new(
            "tx3",
            UndoOperationType::Move,
            "Move 3",
            vec![rec3],
            SystemTime::now(),
        );

        // Record 3 operations
        manager.record_transaction(tx1);
        manager.record_transaction(tx2);
        manager.record_transaction(tx3);

        assert_eq!(manager.undo_stack().len(), 3);
        assert_eq!(manager.redo_stack().len(), 0);
        assert!(manager.can_undo());
        assert!(!manager.can_redo());
        assert_eq!(manager.current_undo_label().as_deref(), Some("Undo Move 3"));

        // 1. Undo tx3
        let undone3 = manager.undo().unwrap().unwrap();
        assert_eq!(undone3.id, "tx3");
        assert_eq!(manager.undo_stack().len(), 2);
        assert_eq!(manager.redo_stack().len(), 1);
        assert_eq!(manager.current_undo_label().as_deref(), Some("Undo Move 2"));
        assert_eq!(manager.current_redo_label().as_deref(), Some("Redo Move 3"));
        assert!(file3_src.exists());
        assert!(!file3_dst.exists());

        // 2. Undo tx2
        let undone2 = manager.undo().unwrap().unwrap();
        assert_eq!(undone2.id, "tx2");
        assert_eq!(manager.undo_stack().len(), 1);
        assert_eq!(manager.redo_stack().len(), 2);
        assert_eq!(manager.current_undo_label().as_deref(), Some("Undo Move 1"));
        assert_eq!(manager.current_redo_label().as_deref(), Some("Redo Move 2"));
        assert!(file2_src.exists());
        assert!(!file2_dst.exists());

        // 3. Redo tx2
        let redone2 = manager.redo().unwrap().unwrap();
        assert_eq!(redone2.id, "tx2");
        assert_eq!(manager.undo_stack().len(), 2);
        assert_eq!(manager.redo_stack().len(), 1);
        assert_eq!(manager.current_undo_label().as_deref(), Some("Undo Move 2"));
        assert_eq!(manager.current_redo_label().as_deref(), Some("Redo Move 3"));
        assert!(!file2_src.exists());
        assert!(file2_dst.exists());

        // 4. A new operation clears redo stack
        let file4_src = dir.path().join("file4_src.txt");
        let file4_dst = dir.path().join("file4_dst.txt");
        fs::write(&file4_dst, "content4").unwrap();
        let stat4 = stat_path(&file4_dst).unwrap();
        let rec4 = FileOperationRecord::new(
            file4_src,
            file4_dst,
            stat4.ino,
            stat4.dev,
            SystemTime::now(),
            None,
        );
        let tx4 = UndoTransaction::new(
            "tx4",
            UndoOperationType::Move,
            "Move 4",
            vec![rec4],
            SystemTime::now(),
        );
        manager.record_transaction(tx4);

        assert_eq!(manager.undo_stack().len(), 3);
        assert_eq!(manager.redo_stack().len(), 0);
        assert!(!manager.can_redo());
        assert_eq!(manager.current_undo_label().as_deref(), Some("Undo Move 4"));

        assert!(notifications.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn test_undo_manager_max_depth_capping() {
        let mut manager = UndoManager::with_depth(3);

        for i in 1..=5 {
            let tx = UndoTransaction::new(
                format!("tx{i}"),
                UndoOperationType::Move,
                format!("Op {i}"),
                Vec::new(),
                SystemTime::now(),
            );
            manager.record_transaction(tx);
        }

        assert_eq!(manager.undo_stack().len(), 3);
        let ids: Vec<_> = manager.undo_stack().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["tx3", "tx4", "tx5"]);
    }

    #[test]
    fn test_purge_records_matching() {
        let mut manager = UndoManager::new();

        let rec1 = FileOperationRecord::new(
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            100,
            1,
            SystemTime::now(),
            None,
        );
        let tx1 = UndoTransaction::new(
            "tx1",
            UndoOperationType::Move,
            "Tx 1",
            vec![rec1],
            SystemTime::now(),
        );

        let rec2 = FileOperationRecord::new(
            PathBuf::from("/c"),
            PathBuf::from("/d"),
            200,
            1,
            SystemTime::now(),
            None,
        );
        let tx2 = UndoTransaction::new(
            "tx2",
            UndoOperationType::Move,
            "Tx 2",
            vec![rec2],
            SystemTime::now(),
        );

        manager.record_transaction(tx1);
        manager.record_transaction(tx2);

        assert_eq!(manager.undo_stack().len(), 2);

        // Purge inode 100
        manager.purge_records_matching(|r| r.inode_number == 100);

        assert_eq!(manager.undo_stack().len(), 1);
        assert_eq!(manager.undo_stack()[0].id, "tx2");
    }
}
