//! SEBI Algo ID Registry for pre-trade risk validation.
//!
//! Under SEBI's Retail Algo Circular (effective August 1, 2025), every
//! algorithmic order must carry a registered `algo_id` that identifies the
//! strategy to the broker and exchange. This registry tracks registered algo
//! IDs and their active/halted status, and is consulted synchronously during
//! pre-trade risk evaluation.

use std::collections::HashMap;

/// Registration status of an algorithm ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlgoRegistrationStatus {
    /// Algo ID is registered and active; orders may be placed.
    Active,
    /// Algo ID is registered but suspended; orders are rejected.
    Suspended {
        /// Reason for suspension
        reason: String,
    },
    /// Algo ID is not registered at all; any order is rejected.
    NotRegistered,
}

/// SEBI Algo ID Registry.
///
/// Tracks which algo IDs are registered with the broker/exchange and their
/// operational status. Consulted synchronously during pre-trade risk checks.
#[derive(Debug, Clone, Default)]
pub struct AlgoIdRegistry {
    /// Map of algo ID to registration status.
    registry: HashMap<String, AlgoRegistrationStatus>,
}

impl AlgoIdRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            registry: HashMap::new(),
        }
    }

    /// Register a new algo ID as active.
    pub fn register(&mut self, algo_id: impl Into<String>) {
        self.registry.insert(algo_id.into(), AlgoRegistrationStatus::Active);
    }

    /// Suspend a registered algo ID.
    pub fn suspend(&mut self, algo_id: &str, reason: impl Into<String>) {
        self.registry.insert(
            algo_id.to_string(),
            AlgoRegistrationStatus::Suspended {
                reason: reason.into(),
            },
        );
    }

    /// Reactivate a suspended algo ID.
    pub fn reactivate(&mut self, algo_id: &str) {
        self.registry
            .insert(algo_id.to_string(), AlgoRegistrationStatus::Active);
    }

    /// Remove an algo ID from the registry.
    pub fn unregister(&mut self, algo_id: &str) {
        self.registry.remove(algo_id);
    }

    /// Check whether an algo ID is registered and active.
    ///
    /// Returns `Ok(())` if the algo is registered and active, or a descriptive
    /// error string if not.
    pub fn is_registered_and_active(&self, algo_id: &str) -> Result<(), String> {
        match self.registry.get(algo_id) {
            Some(AlgoRegistrationStatus::Active) => Ok(()),
            Some(AlgoRegistrationStatus::Suspended { reason }) => {
                Err(format!("algo_id '{algo_id}' is suspended: {reason}"))
            }
            Some(AlgoRegistrationStatus::NotRegistered) => {
                Err(format!("algo_id '{algo_id}' is not registered with the broker"))
            }
            None => Err(format!("algo_id '{algo_id}' is not registered with the broker")),
        }
    }

    /// Returns the number of registered algo IDs.
    pub fn len(&self) -> usize {
        self.registry.len()
    }

    /// Returns true if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }

    /// Returns all registered algo IDs.
    pub fn registered_ids(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_check() {
        let mut registry = AlgoIdRegistry::new();
        assert!(registry.is_empty());

        registry.register("algo-1");
        assert_eq!(registry.len(), 1);
        assert!(registry.is_registered_and_active("algo-1").is_ok());
        assert!(registry
            .is_registered_and_active("algo-unknown")
            .is_err());
    }

    #[test]
    fn test_suspend_and_reactivate() {
        let mut registry = AlgoIdRegistry::new();
        registry.register("algo-1");

        registry.suspend("algo-1", "SEBI regulatory query");
        assert!(matches!(
            registry.is_registered_and_active("algo-1"),
            Err(_)
        ));

        registry.reactivate("algo-1");
        assert!(registry.is_registered_and_active("algo-1").is_ok());
    }

    #[test]
    fn test_unregister() {
        let mut registry = AlgoIdRegistry::new();
        registry.register("algo-1");
        registry.unregister("algo-1");
        assert!(registry.is_empty());
        assert!(registry
            .is_registered_and_active("algo-1")
            .is_err());
    }
}
