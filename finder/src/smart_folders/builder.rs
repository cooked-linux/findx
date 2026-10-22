//! Reactive criteria builder model backing the GTK smart folder criteria bar.

use super::compiler::validate_slice;
use super::types::{
    BooleanMode, CriteriaAttribute, CriteriaGroup, CriteriaOperator, CriteriaSlice, CriteriaValue,
    SmartFolderDefinition,
};

/// Mutable criteria editing model with per-slice validation state.
#[derive(Debug, Clone)]
pub struct CriteriaBuilder {
    definition: SmartFolderDefinition,
    warnings: Vec<String>,
}

impl CriteriaBuilder {
    /// Creates a builder for a new smart folder with a default `Kind is Any` rule.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let mut builder = Self {
            definition: SmartFolderDefinition::new(name, CriteriaGroup::match_all()),
            warnings: Vec::new(),
        };
        builder.revalidate();
        builder
    }

    /// Creates a builder from an existing persisted definition.
    #[must_use]
    pub fn from_definition(definition: SmartFolderDefinition) -> Self {
        let mut builder = Self {
            definition,
            warnings: Vec::new(),
        };
        builder.revalidate();
        builder
    }

    /// Returns the current draft definition.
    #[must_use]
    pub const fn definition(&self) -> &SmartFolderDefinition {
        &self.definition
    }

    /// Consumes the builder and returns the edited definition.
    #[must_use]
    pub fn into_definition(self) -> SmartFolderDefinition {
        self.definition
    }

    /// Returns validation warnings for the current criteria tree.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Returns a mutable reference to the draft definition.
    pub const fn definition_mut(&mut self) -> &mut SmartFolderDefinition {
        &mut self.definition
    }

    /// Returns the number of groups in pre-order (root group is index 0).
    #[must_use]
    pub fn group_count(&self) -> usize {
        fn count(group: &CriteriaGroup) -> usize {
            1 + group.subgroups.iter().map(count).sum::<usize>()
        }
        count(&self.definition.root_criteria)
    }

    /// Re-validates every criteria slice, updating warning badges in place.
    pub fn revalidate(&mut self) {
        let mut warnings = Vec::new();
        revalidate_group(&mut self.definition.root_criteria, &mut warnings);
        self.warnings = warnings;
    }

    /// Appends a sibling rule slice to the group at `group_index`.
    pub fn add_slice(&mut self, group_index: usize) {
        if let Some(group) = self.group_at_mut(group_index) {
            group.slices.push(CriteriaSlice::new(
                CriteriaAttribute::Kind,
                CriteriaOperator::Equals,
                "Any",
            ));
        }
        self.revalidate();
    }

    /// Removes a rule slice from the group at `group_index`.
    pub fn remove_slice(&mut self, group_index: usize, slice_index: usize) {
        if let Some(group) = self.group_at_mut(group_index)
            && slice_index < group.slices.len()
        {
            group.slices.remove(slice_index);
        }
        self.revalidate();
    }

    /// Appends a nested boolean subgroup to the group at `parent_group_index`.
    pub fn add_subgroup(&mut self, parent_group_index: usize) {
        let mode = self
            .group_at(parent_group_index)
            .map_or(BooleanMode::All, |group| group.mode);
        if let Some(group) = self.group_at_mut(parent_group_index) {
            group.subgroups.push(CriteriaGroup::new(mode));
        }
        self.revalidate();
    }

    /// Removes a nested subgroup by its pre-order index.
    pub fn remove_subgroup(&mut self, group_index: usize) {
        if group_index == 0 || group_index >= self.group_count() {
            return;
        }
        remove_group_at(&mut self.definition.root_criteria, group_index);
        self.revalidate();
    }

    /// Updates the boolean combinator of the group at `group_index`.
    pub fn set_boolean_mode(&mut self, group_index: usize, mode: BooleanMode) {
        if let Some(group) = self.group_at_mut(group_index) {
            group.mode = mode;
        }
        self.revalidate();
    }

    /// Updates the attribute of a slice and resets to compatible defaults.
    pub fn set_slice_attribute(
        &mut self,
        group_index: usize,
        slice_index: usize,
        attribute: CriteriaAttribute,
    ) {
        if let Some(slice) = self.slice_at_mut(group_index, slice_index) {
            slice.attribute = attribute;
            let (operator, value) = default_rule(attribute);
            slice.operator = operator;
            slice.value = value;
        }
        self.revalidate();
    }

    /// Updates the comparison operator of a slice.
    pub fn set_slice_operator(
        &mut self,
        group_index: usize,
        slice_index: usize,
        operator: CriteriaOperator,
    ) {
        if let Some(slice) = self.slice_at_mut(group_index, slice_index) {
            slice.operator = operator;
        }
        self.revalidate();
    }

    /// Updates the comparison value of a slice.
    pub fn set_slice_value(
        &mut self,
        group_index: usize,
        slice_index: usize,
        value: CriteriaValue,
    ) {
        if let Some(slice) = self.slice_at_mut(group_index, slice_index) {
            slice.value = value;
        }
        self.revalidate();
    }

    /// Convenience setter for textual criteria values.
    pub fn set_slice_text(
        &mut self,
        group_index: usize,
        slice_index: usize,
        value: impl Into<String>,
    ) {
        self.set_slice_value(group_index, slice_index, CriteriaValue::Text(value.into()));
    }

    /// Convenience setter for rolling day windows.
    pub fn set_slice_days(&mut self, group_index: usize, slice_index: usize, days: u32) {
        self.set_slice_value(group_index, slice_index, CriteriaValue::Days(days));
    }

    /// Convenience setter for size criteria expressed in bytes.
    pub fn set_slice_size_bytes(&mut self, group_index: usize, slice_index: usize, bytes: i64) {
        self.set_slice_value(group_index, slice_index, CriteriaValue::Numeric(bytes));
    }

    /// Returns the group at a pre-order index.
    #[must_use]
    pub fn group_at(&self, group_index: usize) -> Option<&CriteriaGroup> {
        group_at(&self.definition.root_criteria, group_index)
    }

    /// Returns a slice at the supplied group and slice indices.
    #[must_use]
    pub fn slice_at(&self, group_index: usize, slice_index: usize) -> Option<&CriteriaSlice> {
        self.group_at(group_index)
            .and_then(|group| group.slices.get(slice_index))
    }

    fn group_at_mut(&mut self, group_index: usize) -> Option<&mut CriteriaGroup> {
        group_at_mut(&mut self.definition.root_criteria, group_index)
    }

    fn slice_at_mut(
        &mut self,
        group_index: usize,
        slice_index: usize,
    ) -> Option<&mut CriteriaSlice> {
        self.group_at_mut(group_index)
            .and_then(|group| group.slices.get_mut(slice_index))
    }
}

fn default_rule(attribute: CriteriaAttribute) -> (CriteriaOperator, CriteriaValue) {
    match attribute {
        CriteriaAttribute::Kind => (
            CriteriaOperator::Equals,
            CriteriaValue::Text("Any".to_string()),
        ),
        CriteriaAttribute::Name => (
            CriteriaOperator::Contains,
            CriteriaValue::Text(String::new()),
        ),
        CriteriaAttribute::DateModified | CriteriaAttribute::DateCreated => {
            (CriteriaOperator::WithinLastDays, CriteriaValue::Days(7))
        }
        CriteriaAttribute::Tag => (CriteriaOperator::Equals, CriteriaValue::Text(String::new())),
        CriteriaAttribute::Size => (
            CriteriaOperator::GreaterThan,
            CriteriaValue::Numeric(10 * 1024 * 1024),
        ),
    }
}

fn revalidate_group(group: &mut CriteriaGroup, warnings: &mut Vec<String>) {
    for slice in &mut group.slices {
        match validate_slice(slice) {
            Ok(()) => {
                slice.is_valid = true;
                slice.warning_message = None;
            }
            Err(reason) => {
                slice.is_valid = false;
                slice.warning_message = Some(reason.clone());
                warnings.push(format!("{}: {reason}", slice.attribute.label()));
            }
        }
    }
    for subgroup in &mut group.subgroups {
        revalidate_group(subgroup, warnings);
    }
}

fn group_at(group: &CriteriaGroup, mut index: usize) -> Option<&CriteriaGroup> {
    if index == 0 {
        return Some(group);
    }
    index -= 1;
    for subgroup in &group.subgroups {
        let descendant_count = count_groups(subgroup);
        if index < descendant_count {
            return group_at(subgroup, index);
        }
        index -= descendant_count;
    }
    None
}

fn group_at_mut(group: &mut CriteriaGroup, mut index: usize) -> Option<&mut CriteriaGroup> {
    if index == 0 {
        return Some(group);
    }
    index -= 1;
    for subgroup in &mut group.subgroups {
        let descendant_count = count_groups(subgroup);
        if index < descendant_count {
            return group_at_mut(subgroup, index);
        }
        index -= descendant_count;
    }
    None
}

fn count_groups(group: &CriteriaGroup) -> usize {
    1 + group.subgroups.iter().map(count_groups).sum::<usize>()
}

fn remove_group_at(group: &mut CriteriaGroup, index: usize) -> bool {
    fn walk(group: &mut CriteriaGroup, counter: &mut usize, target: usize) -> bool {
        for position in 0..group.subgroups.len() {
            *counter += 1;
            if *counter == target {
                group.subgroups.remove(position);
                return true;
            }
            if walk(&mut group.subgroups[position], counter, target) {
                return true;
            }
        }
        false
    }

    if index == 0 {
        return false;
    }
    let mut counter = 0;
    walk(group, &mut counter, index)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;

    #[test]
    fn test_default_builder_is_valid() {
        let builder = CriteriaBuilder::new("New");
        assert!(builder.warnings().is_empty());
        assert_eq!(builder.definition().root_criteria.slices.len(), 1);
        assert_eq!(builder.group_count(), 1);
    }

    #[test]
    fn test_add_and_remove_slices() {
        let mut builder = CriteriaBuilder::new("Rules");
        builder.add_slice(0);
        assert_eq!(builder.group_count(), 1);
        assert_eq!(builder.definition().root_criteria.slices.len(), 2);

        builder.set_slice_attribute(0, 1, CriteriaAttribute::Name);
        builder.set_slice_text(0, 1, "report");
        assert!(builder.warnings().is_empty());

        builder.remove_slice(0, 1);
        assert_eq!(builder.definition().root_criteria.slices.len(), 1);
    }

    #[test]
    fn test_nested_groups_and_indexing() {
        let mut builder = CriteriaBuilder::new("Compound");
        builder.add_subgroup(0);
        assert_eq!(builder.group_count(), 2);

        builder.set_boolean_mode(0, BooleanMode::Any);
        builder.add_slice(1);
        builder.set_slice_attribute(1, 0, CriteriaAttribute::Tag);
        builder.set_slice_text(1, 0, "Work");
        assert!(builder.warnings().is_empty());

        let definition = builder.definition();
        assert_eq!(definition.root_criteria.mode, BooleanMode::Any);
        assert_eq!(definition.root_criteria.subgroups.len(), 1);

        builder.remove_subgroup(1);
        assert_eq!(builder.group_count(), 1);
    }

    #[test]
    fn test_invalid_name_slice_produces_warning() {
        let mut builder = CriteriaBuilder::new("Warn");
        builder.add_slice(0);
        builder.set_slice_attribute(0, 1, CriteriaAttribute::Name);
        assert_eq!(builder.warnings().len(), 1);
        assert!(!builder.definition().root_criteria.slices[1].is_valid);

        builder.set_slice_text(0, 1, "valid.txt");
        assert!(builder.warnings().is_empty());
        assert!(builder.definition().root_criteria.slices[1].is_valid);
    }

    #[test]
    fn test_size_and_date_defaults() {
        let mut builder = CriteriaBuilder::new("Defaults");
        builder.add_slice(0);
        builder.set_slice_attribute(0, 1, CriteriaAttribute::Size);
        builder.set_slice_size_bytes(0, 1, 5 * 1024 * 1024);
        assert_eq!(
            builder.definition().root_criteria.slices[1].value,
            CriteriaValue::Numeric(5 * 1024 * 1024)
        );

        builder.set_slice_attribute(0, 1, CriteriaAttribute::DateCreated);
        assert_eq!(
            builder.definition().root_criteria.slices[1].operator,
            CriteriaOperator::WithinLastDays
        );
        builder.set_slice_days(0, 1, 14);
        assert_eq!(
            builder.definition().root_criteria.slices[1].value,
            CriteriaValue::Days(14)
        );
    }
}
