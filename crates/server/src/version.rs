pub fn app_version() -> &'static str {
    option_env!("ZMYDIA_BUILD_VERSION")
        .filter(|value| !value.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn application_version_is_valid_semver() {
        semver::Version::parse(super::app_version()).expect("application version must be SemVer");
    }
}
