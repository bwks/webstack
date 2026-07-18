use webstack::AppError;

#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) name: String,
}

impl Item {
    /// Creates an item from trusted persistence data.
    pub(super) fn from_parts(id: String, name: String) -> Self {
        Self { id, name }
    }

    /// Returns a normalized item name or a safe validation error.
    pub(super) fn normalize_name(name: &str) -> Result<String, AppError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(AppError::validation(
                "Item names must contain between 1 and 100 characters.",
            ));
        }
        Ok(name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::Item;

    #[test]
    fn item_names_are_trimmed() {
        assert_eq!(
            Item::normalize_name("  inventory  ").expect("valid name"),
            "inventory"
        );
    }

    #[test]
    fn item_names_must_have_between_one_and_one_hundred_characters() {
        assert!(Item::normalize_name("   ").is_err());
        assert!(Item::normalize_name(&"a".repeat(100)).is_ok());
        assert!(Item::normalize_name(&"a".repeat(101)).is_err());
    }
}
