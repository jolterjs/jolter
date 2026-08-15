pub mod error;
pub mod hash;
pub mod kinds;
pub mod request;
pub mod validation;

pub use error::{RuntimeRequestError, ToolRequestError};
pub use hash::{ToolHash, ToolHashAlgorithm};
pub use kinds::{RuntimeKind, ToolKind};
pub use request::{RuntimeRequest, ToolRequest};

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    #[test]
    fn parses_runtime_request() {
        let request: RuntimeRequest = "node@24".parse().unwrap();
        assert_eq!(request.kind, RuntimeKind::Node);
        assert_eq!(request.selector, "24");
    }

    #[test]
    fn rejects_unknown_runtime() {
        let error = "ruby@3".parse::<RuntimeRequest>().unwrap_err();
        assert!(matches!(
            error,
            RuntimeRequestError::UnsupportedRuntime(runtime) if runtime == "ruby"
        ));
    }

    #[test]
    fn matches_version_prefixes() {
        let version = Version::parse("24.2.1").unwrap();
        assert!(
            "node@24"
                .parse::<RuntimeRequest>()
                .unwrap()
                .matches_version(&version)
        );
        assert!(
            "node@24.x"
                .parse::<RuntimeRequest>()
                .unwrap()
                .matches_version(&version)
        );
        assert!(
            "node@24.2"
                .parse::<RuntimeRequest>()
                .unwrap()
                .matches_version(&version)
        );
        assert!(
            !"node@22"
                .parse::<RuntimeRequest>()
                .unwrap()
                .matches_version(&version)
        );
    }

    #[test]
    fn rejects_invalid_or_unsupported_selectors() {
        assert!("node@24.x.1".parse::<RuntimeRequest>().is_err());
        assert!("node@24-beta".parse::<RuntimeRequest>().is_err());
        assert!(matches!(
            "bun@lts".parse::<RuntimeRequest>(),
            Err(RuntimeRequestError::LtsUnsupported)
        ));
    }

    #[test]
    fn parses_and_matches_tool_requests() {
        let request: ToolRequest = "pnpm@10.x".parse().unwrap();
        assert_eq!(request.kind, ToolKind::Pnpm);
        assert_eq!(request.hash, None);
        assert!(request.matches_version(&Version::new(10, 34, 3)));
        assert!(!request.matches_version(&Version::new(11, 0, 0)));
        assert!("@yarnpkg/cli-dist@4".parse::<ToolRequest>().is_err());
    }

    #[test]
    fn parses_corepack_tool_hashes() {
        let value = format!("pnpm@10.2.0+sha224.{}", "A".repeat(56));
        let request: ToolRequest = value.parse().unwrap();

        assert_eq!(request.selector, "10.2.0");
        assert_eq!(
            request.hash.as_ref().unwrap().algorithm,
            ToolHashAlgorithm::Sha224
        );
        assert_eq!(request.hash.as_ref().unwrap().value, "a".repeat(56));
        assert_eq!(
            request.to_string(),
            format!("pnpm@10.2.0+sha224.{}", "a".repeat(56))
        );
    }

    #[test]
    fn rejects_invalid_corepack_hashes() {
        assert!(matches!(
            format!("pnpm@10+sha224.{}", "a".repeat(56)).parse::<ToolRequest>(),
            Err(ToolRequestError::HashRequiresExactVersion)
        ));
        assert!(matches!(
            "pnpm@10.2.0+md5.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse::<ToolRequest>(),
            Err(ToolRequestError::UnsupportedHashAlgorithm(_))
        ));
        assert!(matches!(
            "pnpm@10.2.0+sha224.deadbeef".parse::<ToolRequest>(),
            Err(ToolRequestError::InvalidHash { .. })
        ));

        let nodejs_req: RuntimeRequest = "nodejs@24".parse().unwrap();
        assert_eq!(nodejs_req.kind, RuntimeKind::Node);
        assert!(!nodejs_req.requires_release_metadata());

        let latest_req = RuntimeRequest::new(RuntimeKind::Node, "latest").unwrap();
        assert!(latest_req.matches_release(&semver::Version::new(24, 0, 0), false));

        let v_req = RuntimeRequest::new(RuntimeKind::Node, "v24.1.0").unwrap();
        assert_eq!(v_req.selector, "24.1.0");

        assert!(matches!(
            RuntimeRequest::new(RuntimeKind::Node, "24 @ 0"),
            Err(RuntimeRequestError::InvalidSelector(_))
        ));

        let lts_req = RuntimeRequest::new(RuntimeKind::Node, "lts").unwrap();
        assert!(lts_req.requires_release_metadata());
        assert!(lts_req.matches_release(&semver::Version::new(24, 0, 0), true));
        assert!(!lts_req.matches_release(&semver::Version::new(24, 0, 0), false));

        let yarnpkg_req: ToolRequest = "yarnpkg@4".parse().unwrap();
        assert_eq!(yarnpkg_req.kind, ToolKind::Yarn);

        assert_eq!(ToolKind::Yarn.registry_package(), "@yarnpkg/cli-dist");
        assert_eq!(ToolKind::Pnpm.registry_package(), "pnpm");
        assert_eq!(ToolKind::Npm.registry_package(), "npm");
        assert_eq!(RuntimeKind::Node.executable_name(), "node");
        assert_eq!(RuntimeKind::Bun.executable_name(), "bun");
        assert_eq!(RuntimeKind::Deno.executable_name(), "deno");

        let alg = hash::ToolHashAlgorithm::Sha224;
        assert_eq!(alg.to_string(), "sha224");
        assert_eq!(alg.hex_length(), 56);
        assert_eq!(hash::ToolHashAlgorithm::Sha1.to_string(), "sha1");
        assert_eq!(hash::ToolHashAlgorithm::Sha1.hex_length(), 40);
        assert_eq!(hash::ToolHashAlgorithm::Sha384.to_string(), "sha384");
        assert_eq!(hash::ToolHashAlgorithm::Sha384.hex_length(), 96);
        assert_eq!(hash::ToolHashAlgorithm::Sha512.to_string(), "sha512");
        assert_eq!(hash::ToolHashAlgorithm::Sha512.hex_length(), 128);

        assert_eq!(
            "sha1".parse::<hash::ToolHashAlgorithm>().unwrap(),
            hash::ToolHashAlgorithm::Sha1
        );
        assert_eq!(
            "sha224".parse::<hash::ToolHashAlgorithm>().unwrap(),
            hash::ToolHashAlgorithm::Sha224
        );
        assert_eq!(
            "sha256".parse::<hash::ToolHashAlgorithm>().unwrap(),
            hash::ToolHashAlgorithm::Sha256
        );
        assert_eq!(
            "sha384".parse::<hash::ToolHashAlgorithm>().unwrap(),
            hash::ToolHashAlgorithm::Sha384
        );
        assert_eq!(
            "sha512".parse::<hash::ToolHashAlgorithm>().unwrap(),
            hash::ToolHashAlgorithm::Sha512
        );

        let hash_struct = hash::ToolHash {
            algorithm: hash::ToolHashAlgorithm::Sha256,
            value: "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
        };
        assert_eq!(
            hash_struct.to_string(),
            "sha256.0000000000000000000000000000000000000000000000000000000000000000"
        );
    }
}
