//! Guardian classes and subclass elements.

use std::fmt;

/// A Guardian class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GuardianClass {
    Titan,
    Hunter,
    Warlock,
    #[default]
    Unknown,
}

impl GuardianClass {
    /// From a manifest/profile `classType` (0 = Titan, 1 = Hunter, 2 = Warlock;
    /// 3 = "any class", which is not a class).
    #[must_use]
    pub fn from_class_type(class_type: u8) -> Self {
        match class_type {
            0 => Self::Titan,
            1 => Self::Hunter,
            2 => Self::Warlock,
            _ => Self::Unknown,
        }
    }

    /// From a PGCR `characterClass` display string.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "Titan" => Self::Titan,
            "Hunter" => Self::Hunter,
            "Warlock" => Self::Warlock,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Titan => "Titan",
            Self::Hunter => "Hunter",
            Self::Warlock => "Warlock",
            Self::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for GuardianClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A subclass element (Light/Darkness damage type, or Prismatic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    Arc,
    Solar,
    Void,
    Stasis,
    Strand,
    Prismatic,
}

impl Element {
    /// Parse the element token Bungie embeds in subclass identifiers, e.g. the
    /// `solar` in `v610.builds.solar.titan.build0` or `titan.solar.supers`.
    /// Prismatic appears as `prism` / `prismatic`.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "arc" => Some(Self::Arc),
            "solar" | "thermal" => Some(Self::Solar),
            "void" => Some(Self::Void),
            "stasis" => Some(Self::Stasis),
            "strand" => Some(Self::Strand),
            "prism" | "prismatic" => Some(Self::Prismatic),
            _ => None,
        }
    }

    /// Find the first element token in a dotted identifier.
    #[must_use]
    pub fn from_identifier(identifier: &str) -> Option<Self> {
        identifier
            .split(['.', '_'])
            .find_map(|t| Self::from_token(&t.to_ascii_lowercase()))
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Arc => "Arc",
            Self::Solar => "Solar",
            Self::Void => "Void",
            Self::Stasis => "Stasis",
            Self::Strand => "Strand",
            Self::Prismatic => "Prismatic",
        }
    }
}

impl fmt::Display for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_parsing() {
        assert_eq!(GuardianClass::from_class_type(2), GuardianClass::Warlock);
        assert_eq!(GuardianClass::from_class_type(3), GuardianClass::Unknown);
        assert_eq!(GuardianClass::from_name("Hunter"), GuardianClass::Hunter);
        assert_eq!(GuardianClass::from_name("hunter"), GuardianClass::Unknown);
    }

    #[test]
    fn element_from_identifiers() {
        assert_eq!(
            Element::from_identifier("v610.builds.solar.titan.build0"),
            Some(Element::Solar)
        );
        assert_eq!(
            Element::from_identifier("hunter.prism.supers"),
            Some(Element::Prismatic)
        );
        assert_eq!(
            Element::from_identifier("shared.stasis.trinkets"),
            Some(Element::Stasis)
        );
        assert_eq!(Element::from_identifier("warlock.class_movement"), None);
    }
}
