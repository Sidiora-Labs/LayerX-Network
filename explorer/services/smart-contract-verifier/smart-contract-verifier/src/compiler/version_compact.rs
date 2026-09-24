use super::fetcher::Version;
use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub struct CompactVersion(semver::Version);

impl Display for CompactVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl FromStr for CompactVersion {
    type Err = semver::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(semver::Version::from_str(s.trim_start_matches('v'))?))
    }
}

impl Version for CompactVersion {
    fn to_semver(&self) -> &semver::Version {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rand::{rngs::StdRng, seq::SliceRandom, thread_rng, Rng, SeedableRng};

    fn check_parsing<T: FromStr + ToString>(ver_str: &str) -> T
    where
        <T as FromStr>::Err: std::fmt::Debug,
    {
        T::from_str(ver_str).unwrap()
    }

    #[test]
    fn parse() {
        let ver = check_parsing::<CompactVersion>("v1.4.1");
        assert_eq!(ver.0, semver::Version::new(1, 4, 1));
        let ver = check_parsing::<CompactVersion>("1.4.1");
        assert_eq!(ver.0, semver::Version::new(1, 4, 1));
        check_parsing::<CompactVersion>("v0.0.0");
        check_parsing::<CompactVersion>("v123456789.987654321.0");
        check_parsing::<CompactVersion>("v1.2.3");
        check_parsing::<CompactVersion>("v3.2.1");
    }

    #[test]
    fn display_version() {
        for (initial, expected) in [("v1.4.1", "v1.4.1"), ("1.4.1", "v1.4.1")] {
            let version = check_parsing::<CompactVersion>(initial);
            assert_eq!(version.to_string(), expected,);
        }
    }

    #[test]
    fn order_versions() {
        let ver = check_parsing::<CompactVersion>;

        assert!(ver("v1.3.1") > ver("v1.3.0"));
        assert!(ver("v1.3.0") > ver("v1.2.9"));
        assert!(ver("v1.13.0") > ver("v1.2.9"));
        assert!(ver("v1.13.0") > ver("v1.3.1"));
    }

    fn test_shuffle_and_sort(sorted: Vec<&str>, times: usize) {
        let sorted_versions: Vec<CompactVersion> = sorted
            .iter()
            .map(|s| CompactVersion::from_str(s).expect("invalid version"))
            .collect();
        // check, that array is indeed sorted
        assert!(sorted_versions.windows(2).all(|vals| vals[0] <= vals[1]));
        let seed = thread_rng().gen();
        let mut r = StdRng::seed_from_u64(seed);
        let mut shuffled_versions = sorted_versions;
        for i in 0..times {
            shuffled_versions.shuffle(&mut r);
            shuffled_versions.sort();
            let shuffled: Vec<String> = shuffled_versions.iter().map(|v| v.to_string()).collect();
            // we compare vec of strings, because in case of wrong order
            // test will show unreadable error (it will print a lot of large structures)
            assert_eq!(shuffled, sorted, "seed={}, i={}", seed, i);
        }
    }

    #[test]
    fn sort_all_versions() {
        test_shuffle_and_sort(vec!["v1.2.9", "v1.3.0", "v1.3.1", "v1.13.0"], 100);
    }
}
