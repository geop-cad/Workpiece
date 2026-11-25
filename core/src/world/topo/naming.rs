//! Persistent (topological) names: a bijection between entity ids and
//! strings.
//!
//! Arena ids are an implementation detail that changes on every rebuild;
//! names are what features, profiles and the user refer to. Operations
//! assign names from their provenance (e.g. `"extrude1/side/3"`,
//! `"extrude1/cap/top"`), and a later rebuild assigns the same names to
//! the corresponding new entities, so references survive. A name that does
//! not resolve is reported as lost, never guessed.
//!
//! The map is a bijection: one name per id, one id per name.

use std::collections::BTreeMap;

use super::entity::EntityId;

/// Why a name could not be assigned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NamingError {
    /// The name already belongs to another entity.
    #[error("name {name:?} already names {owner:?}")]
    NameTaken {
        /// The name.
        name: String,
        /// Its current owner.
        owner: EntityId,
    },
    /// The entity already has a different name.
    #[error("{id:?} is already named {name:?}")]
    AlreadyNamed {
        /// The entity.
        id: EntityId,
        /// Its current name.
        name: String,
    },
    /// The empty string is not a name.
    #[error("names must not be empty")]
    EmptyName,
}

/// A bidirectional `EntityId ↔ String` map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameMap {
    by_id: BTreeMap<EntityId, String>,
    by_name: BTreeMap<String, EntityId>,
    /// The names an entity is known by besides its own: those of what
    /// was made one with it (two faces in one plane, joined).
    also: BTreeMap<String, EntityId>,
}

impl NameMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of named entities.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// `true` when nothing is named.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Names `id`. Re-assigning the same pair is a no-op; anything that
    /// would break the bijection is refused (use [`Self::rename`]).
    pub fn insert(&mut self, id: EntityId, name: impl Into<String>) -> Result<(), NamingError> {
        let name = name.into();
        if name.is_empty() {
            return Err(NamingError::EmptyName);
        }
        if let Some(&owner) = self.by_name.get(&name) {
            return if owner == id {
                Ok(())
            } else {
                Err(NamingError::NameTaken { name, owner })
            };
        }
        if let Some(existing) = self.by_id.get(&id) {
            return Err(NamingError::AlreadyNamed {
                id,
                name: existing.clone(),
            });
        }
        self.by_name.insert(name.clone(), id);
        self.by_id.insert(id, name);
        Ok(())
    }

    /// Gives `id` the name `name`, dropping any previous name of `id`.
    /// Refused when `name` belongs to another entity.
    pub fn rename(&mut self, id: EntityId, name: impl Into<String>) -> Result<(), NamingError> {
        let name = name.into();
        if name.is_empty() {
            return Err(NamingError::EmptyName);
        }
        match self.by_name.get(&name) {
            Some(&owner) if owner == id => return Ok(()),
            Some(&owner) => return Err(NamingError::NameTaken { name, owner }),
            None => {}
        }
        self.remove_id(id);
        self.insert(id, name)
    }

    /// The name of `id`.
    pub fn name_of(&self, id: impl Into<EntityId>) -> Option<&str> {
        self.by_id.get(&id.into()).map(String::as_str)
    }

    /// The entity called `name`.
    pub fn id_of(&self, name: &str) -> Option<EntityId> {
        self.by_name.get(name).copied()
    }

    /// The entity `name` refers to: the one called so, or else the one
    /// also known by that name.
    pub fn known(&self, name: &str) -> Option<EntityId> {
        self.id_of(name).or_else(|| self.also.get(name).copied())
    }

    /// Makes `id` also known by `name` (unless something else is known
    /// by it already, or it is empty).
    pub fn also(&mut self, id: impl Into<EntityId>, name: impl Into<String>) {
        let name = name.into();
        if !name.is_empty() && !self.by_name.contains_key(&name) {
            self.also.entry(name).or_insert_with(|| id.into());
        }
    }

    /// The names `id` is known by besides its own, in name order.
    pub fn also_of(&self, id: impl Into<EntityId>) -> Vec<&str> {
        let id = id.into();
        self.also
            .iter()
            .filter(|(_, owner)| **owner == id)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Forgets the name of `id` (and what else it is known by),
    /// returning it.
    pub fn remove_id(&mut self, id: impl Into<EntityId>) -> Option<String> {
        let id = id.into();
        self.also.retain(|_, owner| *owner != id);
        let name = self.by_id.remove(&id)?;
        self.by_name.remove(&name);
        Some(name)
    }

    /// Forgets `name`, returning the entity it named.
    pub fn remove_name(&mut self, name: &str) -> Option<EntityId> {
        let id = self.by_name.remove(name)?;
        self.by_id.remove(&id);
        self.also.retain(|_, owner| *owner != id);
        Some(id)
    }

    /// All `(name, id)` pairs in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, EntityId)> {
        self.by_name.iter().map(|(n, id)| (n.as_str(), *id))
    }

    /// All `(name, id)` pairs whose name starts with `prefix`, in name
    /// order — e.g. everything an operation called `"extrude1/"` made.
    pub fn with_prefix<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a str, EntityId)> + 'a {
        self.by_name
            .range::<str, _>((
                core::ops::Bound::Included(prefix),
                core::ops::Bound::Unbounded,
            ))
            .take_while(move |(n, _)| n.starts_with(prefix))
            .map(|(n, id)| (n.as_str(), *id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::topo::arena::Id;

    fn v(i: u32) -> EntityId {
        EntityId::Vertex(Id::from_raw(i))
    }

    #[test]
    fn bijection_is_kept() -> anyhow::Result<()> {
        let mut m = NameMap::new();
        m.insert(v(0), "a")?;
        m.insert(v(0), "a")?;
        assert!(matches!(
            m.insert(v(1), "a"),
            Err(NamingError::NameTaken { .. })
        ));
        assert!(matches!(
            m.insert(v(0), "b"),
            Err(NamingError::AlreadyNamed { .. })
        ));
        m.rename(v(0), "b")?;
        assert_eq!(m.id_of("a"), None);
        assert_eq!(m.id_of("b"), Some(v(0)));
        assert_eq!(m.name_of(v(0)), Some("b"));
        assert_eq!(m.len(), 1);
        Ok(())
    }

    #[test]
    fn prefix_query() -> anyhow::Result<()> {
        let mut m = NameMap::new();
        m.insert(v(0), "ex1/side/0")?;
        m.insert(v(1), "ex1/side/1")?;
        m.insert(v(2), "ex2/side/0")?;
        let got: Vec<_> = m.with_prefix("ex1/").map(|(n, _)| n).collect();
        assert_eq!(got, ["ex1/side/0", "ex1/side/1"]);
        Ok(())
    }
}
