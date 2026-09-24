use super::fetcher::Version;
use chrono::NaiveDate;
use semver::{BuildMetadata, Prerelease};
use std::{cmp::Ordering, fmt::Display, str::FromStr};
use thiserror::Error;

const DATE_FORMAT: &str = "%Y.%-m.%-d";

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("error parsing the string: {0}")]
    Parse(String),
    #[error("couldn't parse commit hash")]
    CommitHash(hex::FromHexError),
    #[error("cannot parse semver: {0}")]
    Semver(#[from] semver::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReleaseVersion {
    pub version: semver::Version,
    pub commit: String,
}

impl FromStr for ReleaseVersion {
    type Err = ParseError;

    /// Parses release version from string formated as
    /// `(v)*VERSION*(-*PRERELEASE*)+commit.*COMMITHASH*`, examples:
    /// `v0.8.9+commit.e5eed63a`
    /// `0.8.4+commit.dea1b9ec`
    /// `0.8.4-beta.16+commit.dea1d`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_prefix, major, minor, patch, pre, commit) = sscanf::scanf!(
            s,
            "{:/v?/}{}.{}.{}{:/(?:-.*)?/}+commit.{:/[A-Fa-f0-9]+/}",
            String,
            u64,
            u64,
            u64,
            String,
            String,
        )
        .map_err(|e| ParseError::Parse(format!("{e:?}")))?;
        let version = semver::Version {
            major,
            minor,
            patch,
            pre: Prerelease::new(pre.trim_start_matches('-'))?,
            build: BuildMetadata::EMPTY,
        };
        Ok(Self { version, commit })
    }
}

impl Display for ReleaseVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}+commit.{}", self.version, self.commit)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NightlyVersion {
    pub version: semver::Version,
    pub date: NaiveDate,
    pub commit: String,
}

impl FromStr for NightlyVersion {
    type Err = ParseError;

    /// Parses nigthly version from string formated as
    /// `(v)*VERSION*-nightly.*DATE*+commit.*COMMITHASH*`, examples:
    /// `v0.8.8-nightly.2021.9.9+commit.dea1b9ec`
    /// `0.8.4-nightly.2021.9.9+commit.e5eed63a10`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_prefix, major, minor, patch, date, commit) = sscanf::scanf!(
            s,
            "{:/v?/}{}.{}.{}-nightly.{}+commit.{:/[A-Fa-f0-9]+/}",
            String,
            u64,
            u64,
            u64,
            String,
            String,
        )
        .map_err(|e| ParseError::Parse(format!("{e:?}")))?;
        let version = semver::Version::new(major, minor, patch);
        let date = NaiveDate::parse_from_str(&date, DATE_FORMAT)
            .map_err(|e| ParseError::Parse(e.to_string()))?;
        Ok(Self {
            version,
            date,
            commit,
        })
    }
}

impl Display for NightlyVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "v{}-nightly.{}+commit.{}",
            self.version,
            self.date.format(DATE_FORMAT),
            self.commit
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DetailedVersion {
    Release(ReleaseVersion),
    Nightly(NightlyVersion),
}

impl DetailedVersion {
    pub fn version(&self) -> &semver::Version {
        match self {
            DetailedVersion::Nightly(v) => &v.version,
            DetailedVersion::Release(v) => &v.version,
        }
    }

    pub fn is_release(&self) -> bool {
        matches!(self, DetailedVersion::Release(_))
    }

    pub fn date(&self) -> Option<&NaiveDate> {
        match self {
            DetailedVersion::Nightly(v) => Some(&v.date),
            DetailedVersion::Release(_) => None,
        }
    }

    pub fn commit(&self) -> &str {
        match self {
            DetailedVersion::Nightly(v) => &v.commit,
            DetailedVersion::Release(v) => &v.commit,
        }
    }
}

impl Version for DetailedVersion {
    fn to_semver(&self) -> &semver::Version {
        self.version()
    }
}

impl FromStr for DetailedVersion {
    type Err = ParseError;

    /// Parses compiler version
    /// If version contains "nightly", tries to parse it as a nightly version
    /// Else tries to parse it as a release version
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("nightly") {
            Ok(Self::Nightly(NightlyVersion::from_str(s)?))
        } else {
            Ok(Self::Release(ReleaseVersion::from_str(s)?))
        }
    }
}

impl Display for DetailedVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetailedVersion::Release(v) => v.fmt(f),
            DetailedVersion::Nightly(v) => v.fmt(f),
        }
    }
}

impl Ord for DetailedVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.version(),
            self.is_release(),
            self.date(),
            self.commit(),
        )
            .cmp(&(
                other.version(),
                other.is_release(),
                other.date(),
                other.commit(),
            ))
    }
}

impl PartialOrd for DetailedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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
    fn parse_release() {
        let ver = check_parsing::<ReleaseVersion>("v0.8.9+commit.e5eed63a");
        assert_eq!(ver.version, semver::Version::new(0, 8, 9));
        assert_eq!(ver.commit, "e5eed63a");
        check_parsing::<ReleaseVersion>("0.8.9+commit.00000000");
        check_parsing::<ReleaseVersion>("v0.0.0+commit.00000000");
        check_parsing::<ReleaseVersion>("v123456789.987654321.0+commit.ffffffff");
        check_parsing::<ReleaseVersion>("v1.2.3+commit.01234567");
        check_parsing::<ReleaseVersion>("v3.2.1+commit.89abcdef");
        check_parsing::<ReleaseVersion>("0.1.0-beta.16+commit.5e4a94a");
        check_parsing::<ReleaseVersion>("0.1.0-beta.17+commit.0671b7b");
    }

    #[test]
    fn parse_invalid_release() {
        ReleaseVersion::from_str("").unwrap_err();
        ReleaseVersion::from_str("sometext").unwrap_err();
        ReleaseVersion::from_str("0.8+commit.deadbeef").unwrap_err();
        ReleaseVersion::from_str("v0.8+commit.deadbeef").unwrap_err();
        ReleaseVersion::from_str("v0.8.9commit.deadbeef").unwrap_err();
        ReleaseVersion::from_str("v0.8.9+commitdeadbeef").unwrap_err();
        ReleaseVersion::from_str("v+commit.deadbeef").unwrap_err();
        ReleaseVersion::from_str("v0.8.9+commit.").unwrap_err();
        ReleaseVersion::from_str("v0.8.9+commit.deadbev").unwrap_err();
        ReleaseVersion::from_str("vv0.8.9+commit.deadbeef").unwrap_err();
        ReleaseVersion::from_str("-v0.8.9+commit.deadbeef").unwrap_err();
        ReleaseVersion::from_str("v0.8.9+commit.alivebee").unwrap_err();
        ReleaseVersion::from_str("0.1.0beta.17+commit.0671b7b").unwrap_err();
    }

    #[test]
    fn parse_nightly() {
        let ver = check_parsing::<NightlyVersion>("v10.8.9-nightly.2021.9.11+commit.e5eed63a");
        assert_eq!(ver.version, semver::Version::new(10, 8, 9));
        assert_eq!(ver.date, NaiveDate::from_ymd_opt(2021, 9, 11).unwrap());
        assert_eq!(ver.commit, "e5eed63a");
        check_parsing::<NightlyVersion>("v0.0.0-nightly.1990.1.1+commit.00000000");
        check_parsing::<NightlyVersion>(
            "v123456789.987654321.0-nightly.2100.12.30+commit.ffffffff",
        );
        let ver = check_parsing::<NightlyVersion>("0.0.0-nightly.1990.1.1+commit.00000000");
        assert_eq!(ver.version, semver::Version::new(0, 0, 0));
    }

    #[test]
    fn parse_invalid_nightly() {
        NightlyVersion::from_str("").unwrap_err();
        NightlyVersion::from_str("sometext").unwrap_err();
        NightlyVersion::from_str("v0.8-nightly.2021.9.11+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("v0.8-nightly.2021.9.11+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("v0.8.9.2021.9.11+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("v0.8.9-nightly.+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("v-nightly.2021.9.11+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("v0.8.9-nightly.2021.9.11+commit.").unwrap_err();
        NightlyVersion::from_str("vv0.8.9-nighly.2021.9.11+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("-v0.8.9-nighly.2021.9.11+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("solc-v0.8.9-nighly.2021.9.11+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("v0.8.9-nighly.2021.9.11+commit.e5eed63a").unwrap_err();
        NightlyVersion::from_str("v0.8.9+commit.deadbeef").unwrap_err();
    }

    #[test]
    fn parse_version() {
        assert_eq!(
            check_parsing::<DetailedVersion>("v0.8.9+commit.e5eed63a"),
            DetailedVersion::Release(ReleaseVersion::from_str("v0.8.9+commit.e5eed63a").unwrap())
        );
        assert_eq!(
            check_parsing::<DetailedVersion>("v0.8.9-nightly.2021.9.11+commit.e5eed63a"),
            DetailedVersion::Nightly(
                NightlyVersion::from_str("v0.8.9-nightly.2021.9.11+commit.e5eed63a").unwrap()
            )
        );
        assert_eq!(
            check_parsing::<DetailedVersion>("0.1.0-beta.16+commit.5e4a94a"),
            DetailedVersion::Release(
                ReleaseVersion::from_str("0.1.0-beta.16+commit.5e4a94a").unwrap()
            )
        );
    }

    #[test]
    fn display_version() {
        for (initial, expected) in [
            ("v0.8.9+commit.e5eed63a", "v0.8.9+commit.e5eed63a"),
            (
                "0.8.9-nightly.2021.09.11+commit.e5ee",
                "v0.8.9-nightly.2021.9.11+commit.e5ee",
            ),
            (
                "0.1.0-beta.16+commit.5e4a94a",
                "v0.1.0-beta.16+commit.5e4a94a",
            ),
        ] {
            let version = check_parsing::<DetailedVersion>(initial);
            assert_eq!(version.to_string(), expected,);
        }
    }

    #[test]
    fn order_versions() {
        let ver = check_parsing::<DetailedVersion>;

        // Release only
        assert!(ver("v0.8.10+commit.fc410830") > ver("v0.8.9+commit.e5eed63a"));
        // note: version with different hashes shouldn't be equal to implement sorting
        assert!(ver("v0.8.9+commit.fc410830") > ver("v0.8.9+commit.e5eed63a"));
        assert!(ver("v0.8.9+commit.e5eed63a") == ver("v0.8.9+commit.e5eed63a"));

        // Nighly only
        assert!(
            ver("v0.8.15-nightly.2022.4.4+commit.fd763fa6")
                > ver("v0.8.2-nightly.2022.3.16+commit.10b581b8")
        );
        assert!(
            ver("v0.8.15-nightly.2022.4.4+commit.fd763fa6")
                > ver("v0.8.14-nightly.2022.3.16+commit.10b581b8")
        );
        assert!(
            ver("v0.8.14-nightly.2022.4.4+commit.fd763fa6")
                > ver("v0.8.14-nightly.2022.3.16+commit.10b581b8")
        );
        assert!(
            ver("v0.8.14-nightly.2022.4.4+commit.fd763fa6")
                > ver("v0.8.14-nightly.2022.4.4+commit.10b581b8")
        );
        assert!(
            ver("v0.8.14-nightly.2022.4.4+commit.fd763fa6")
                == ver("v0.8.14-nightly.2022.4.4+commit.fd763fa6")
        );

        // All
        assert!(ver("v0.5.2+commit.1df8f40c") > ver("v0.5.2-nightly.2018.12.19+commit.88750920"));
        assert!(ver("v0.5.14+commit.1df8f40c") > ver("v0.5.2-nightly.2018.12.19+commit.88750920"));
        assert!(ver("v0.5.14-nightly.2019.12.9+commit.d6667560") > ver("v0.5.2+commit.1df8f40c"));
    }

    fn test_shuffle_and_sort(sorted: Vec<&str>, times: usize) {
        let sorted_versions: Vec<DetailedVersion> = sorted
            .iter()
            .map(|s| DetailedVersion::from_str(s).expect("invalid version"))
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
    fn sort_release_versions() {
        test_shuffle_and_sort(
            vec![
                "v0.8.9+commit.e5eed63a",
                "v0.8.10+commit.fc410830",
                "v0.8.11+commit.d7f03943",
                "v0.8.12+commit.f00d7308",
                "v0.8.13+commit.abaa5c0e",
                "v0.8.14+commit.80d49f37",
                "v0.8.15+commit.e14f2714",
            ],
            50,
        );
    }

    #[test]
    fn sort_nightly_versions() {
        test_shuffle_and_sort(
            vec![
                "v0.8.14-nightly.2022.3.16+commit.10b581b8",
                "v0.8.14-nightly.2022.3.17+commit.430ecb6e",
                "v0.8.14-nightly.2022.3.21+commit.43f29c00",
                "v0.8.14-nightly.2022.3.23+commit.b35cda59",
                "v0.8.14-nightly.2022.3.24+commit.c4909e99",
                "v0.8.14-nightly.2022.4.4+commit.fd763fa6",
                "v0.8.14-nightly.2022.4.5+commit.34dd30d7",
                "v0.8.14-nightly.2022.4.6+commit.31b54857",
                "v0.8.14-nightly.2022.4.7+commit.15c2a33e",
                "v0.8.14-nightly.2022.4.8+commit.d9c6ceca",
                "v0.8.14-nightly.2022.4.10+commit.0b811943",
                "v0.8.14-nightly.2022.4.11+commit.9e92c7a4",
                "v0.8.14-nightly.2022.4.13+commit.25923c1f",
                "v0.8.14-nightly.2022.4.14+commit.55917405",
                "v0.8.14-nightly.2022.4.25+commit.fbecdbe7",
                "v0.8.14-nightly.2022.4.28+commit.d55b84ff",
                "v0.8.14-nightly.2022.5.2+commit.3e3e73e3",
                "v0.8.14-nightly.2022.5.4+commit.84c64edf",
                "v0.8.14-nightly.2022.5.5+commit.1dba6aaf",
                "v0.8.14-nightly.2022.5.9+commit.463e4175",
                "v0.8.14-nightly.2022.5.10+commit.9f6d3dea",
                "v0.8.14-nightly.2022.5.11+commit.0c0ff4fc",
                "v0.8.14-nightly.2022.5.12+commit.aafda389",
                "v0.8.14-nightly.2022.5.13+commit.a3bd01d9",
                "v0.8.14-nightly.2022.5.17+commit.80d49f37",
                "v0.8.15-nightly.2022.5.18+commit.de7daaa2",
                "v0.8.15-nightly.2022.5.19+commit.0cb95902",
                "v0.8.15-nightly.2022.5.20+commit.02567fd3",
                "v0.8.15-nightly.2022.5.23+commit.21591531",
                "v0.8.15-nightly.2022.5.25+commit.fdc3c8ee",
                "v0.8.15-nightly.2022.5.27+commit.095cc647",
                "v0.8.15-nightly.2022.5.31+commit.baf56aff",
                "v0.8.15-nightly.2022.6.1+commit.3f84837e",
                "v0.8.15-nightly.2022.6.2+commit.035f6abb",
                "v0.8.15-nightly.2022.6.6+commit.3948391c",
                "v0.8.15-nightly.2022.6.7+commit.8c87f58f",
                "v0.8.15-nightly.2022.6.8+commit.9b220a20",
                "v0.8.15-nightly.2022.6.9+commit.80f6a13d",
                "v0.8.15-nightly.2022.6.10+commit.efcbc79b",
                "v0.8.15-nightly.2022.6.13+commit.82e5339d",
                "v0.8.15-nightly.2022.6.14+commit.dccc06cc",
                "v0.8.16-nightly.2022.6.15+commit.f904bb06",
                "v0.8.16-nightly.2022.6.16+commit.b80f4baa",
                "v0.8.16-nightly.2022.6.17+commit.be470c16",
            ],
            50,
        );
    }

    #[test]
    fn sort_all_versions() {
        test_shuffle_and_sort(
            vec![
                "v0.5.2-nightly.2018.12.3+commit.e6a01d26",
                "v0.5.2-nightly.2018.12.4+commit.e49f37be",
                "v0.5.2-nightly.2018.12.5+commit.6efe2a52",
                "v0.5.2-nightly.2018.12.6+commit.5a08ae5e",
                "v0.5.2-nightly.2018.12.7+commit.52ff3c94",
                "v0.5.2-nightly.2018.12.10+commit.6240d9e7",
                "v0.5.2-nightly.2018.12.11+commit.599760b6",
                "v0.5.2-nightly.2018.12.12+commit.85291bcb",
                "v0.5.2-nightly.2018.12.13+commit.b3e2ba15",
                "v0.5.2-nightly.2018.12.17+commit.12874029",
                "v0.5.2-nightly.2018.12.18+commit.4b43aeca",
                "v0.5.2-nightly.2018.12.19+commit.88750920",
                "v0.5.2+commit.1df8f40c",
                "v0.5.14-nightly.2019.12.9+commit.d6667560",
                "v0.5.15+commit.6a57276f",
                "v0.5.16+commit.9c3226ce",
                "v0.5.17+commit.d19bba13",
                "v0.6.2-nightly.2020.1.8+commit.12b52ae6",
                "v0.6.2-nightly.2020.1.9+commit.17158995",
                "v0.6.2-nightly.2020.1.13+commit.408458b7",
                "v0.6.2-nightly.2020.1.14+commit.6dbadf69",
                "v0.6.2-nightly.2020.1.15+commit.9d9a7ebe",
                "v0.6.2-nightly.2020.1.16+commit.3d4a2219",
                "v0.6.2-nightly.2020.1.17+commit.92908f52",
                "v0.6.2-nightly.2020.1.20+commit.470c19eb",
                "v0.6.2-nightly.2020.1.22+commit.641bb815",
                "v0.6.2-nightly.2020.1.23+commit.3add37a2",
                "v0.6.2-nightly.2020.1.27+commit.1bdb409b",
                "v0.6.2+commit.bacdbe57",
                "v0.6.3-nightly.2020.1.27+commit.8809d4bb",
                "v0.6.3-nightly.2020.1.28+commit.2d3bd91d",
                "v0.6.3-nightly.2020.1.29+commit.01eb9a5b",
                "v0.6.3-nightly.2020.1.30+commit.ad98bf0f",
                "v0.6.3-nightly.2020.1.31+commit.b6190e06",
                "v0.6.3+commit.8dda9521",
                "v0.6.12+commit.27d51765",
            ],
            100,
        );
    }
}
