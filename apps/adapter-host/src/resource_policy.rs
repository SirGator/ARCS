//! Operator-defined admission limits; quantities describe logical capacity.
use crate::HostError;
use std::collections::BTreeMap;

pub type ResourceQuantities = BTreeMap<String, u32>;

pub(crate) fn validate(
    pools: &ResourceQuantities,
    claims: Option<&ResourceQuantities>,
) -> Result<(), HostError> {
    let valid_name = |name: &str| {
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    };
    if pools.len() > 64
        || pools
            .iter()
            .any(|(name, units)| !valid_name(name) || !(1..=1_000_000_000).contains(units))
    {
        return Err(HostError::InvalidConfig("resource_pools requires at most 64 named capacities of 1..1000000000 units (names: 1..64 ASCII letters, digits, '.', '_' or '-')".into()));
    }
    if let Some(claims) = claims
        && (claims.len() > 64
            || claims.iter().any(|(name, units)| {
                *units == 0 || pools.get(name).is_none_or(|capacity| units > capacity)
            }))
    {
        return Err(HostError::InvalidConfig(
            "reasoning_resources must request positive units within declared resource_pools".into(),
        ));
    }
    Ok(())
}
