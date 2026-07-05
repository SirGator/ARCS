/**
 * @file policy_service.hpp
 * @brief Loads the current and previous policy for a scope via a pluggable
 *        repository, with a demo in-memory implementation provided.
 */

#pragma once

#include <memory>
#include <optional>

#include "core/services/common.hpp"
#include "store/store.hpp"

namespace arcs::core::services {

/**
 * @brief Abstract source of policy snapshots for a scope, allowing the
 *        policy backing store to be swapped (e.g. real store vs demo data).
 */
class IPolicyRepository {
public:
    virtual ~IPolicyRepository() = default;

    /**
     * @brief Loads the current and previous policy artifacts for a scope.
     * @param scope Scope to load policy for.
     * @return The policy snapshot for the scope.
     */
    virtual PolicySnapshot load_policy_snapshot(const std::string& scope) const = 0;
};

/**
 * @brief Demo/in-memory `IPolicyRepository` implementation used when no real
 *        policy store is wired up.
 */
class BootstrapPolicyRepository final : public IPolicyRepository {
public:
    /** @brief Loads a hardcoded bootstrap policy snapshot for the scope. */
    PolicySnapshot load_policy_snapshot(const std::string& scope) const override;
};

class StorePolicyRepository final : public IPolicyRepository {
public:
    explicit StorePolicyRepository(const arcs::store::IStore& store);

    PolicySnapshot load_policy_snapshot(const std::string& scope) const override;

private:
    const arcs::store::IStore& store_;
};

class FallbackPolicyRepository final : public IPolicyRepository {
public:
    FallbackPolicyRepository(
        std::unique_ptr<IPolicyRepository> primary,
        std::unique_ptr<IPolicyRepository> fallback);

    PolicySnapshot load_policy_snapshot(const std::string& scope) const override;

private:
    std::unique_ptr<IPolicyRepository> primary_;
    std::unique_ptr<IPolicyRepository> fallback_;
};

/**
 * @brief Facade over an `IPolicyRepository` that exposes convenience
 *        accessors for the current and previous policy of a scope.
 */
class PolicyService {
public:
    /**
     * @brief Constructs the service over the given policy repository.
     * @param repository Repository to delegate policy lookups to.
     */
    explicit PolicyService(const IPolicyRepository& repository);

    /**
     * @brief Loads the current and previous policy artifacts for a scope.
     * @param scope Scope to load policy for.
     * @return The policy snapshot for the scope.
     */
    PolicySnapshot load_policy_snapshot(const std::string& scope) const;

    /**
     * @brief Returns the current policy artifact for a scope.
     * @param scope Scope to look up.
     * @return The current policy artifact.
     */
    arcs::artifact::ArtifactVersion current_policy_for_scope(const std::string& scope) const;

    /**
     * @brief Returns the previous policy artifact for a scope, if any.
     * @param scope Scope to look up.
     * @return The previous policy artifact, or empty if there was none.
     */
    std::optional<arcs::artifact::ArtifactVersion> previous_policy_for_scope(const std::string& scope) const;

private:
    const IPolicyRepository& repository_;
};

} // namespace arcs::core::services
