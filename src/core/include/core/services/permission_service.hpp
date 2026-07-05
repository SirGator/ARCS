/**
 * @file permission_service.hpp
 * @brief Resolves the effective permissions for a principal/scope pair,
 *        supporting a demo-mode override.
 */

#pragma once

#include <memory>
#include <string>
#include <vector>

#include "artifact/artifact.hpp"
#include "reducer/effective_permissions.hpp"
#include "reducer/time_source.hpp"
#include "store/store.hpp"

namespace arcs::core::services {

class IPermissionSource {
public:
    virtual ~IPermissionSource() = default;

    virtual std::vector<arcs::artifact::ArtifactVersion> load_permission_grants(
        const std::string& principal,
        const std::string& scope) const = 0;
};

class StorePermissionSource final : public IPermissionSource {
public:
    explicit StorePermissionSource(const arcs::store::IStore& store);

    std::vector<arcs::artifact::ArtifactVersion> load_permission_grants(
        const std::string& principal,
        const std::string& scope) const override;

private:
    const arcs::store::IStore& store_;
};

class DemoPermissionSource final : public IPermissionSource {
public:
    std::vector<arcs::artifact::ArtifactVersion> load_permission_grants(
        const std::string& principal,
        const std::string& scope) const override;
};

class CompositePermissionSource final : public IPermissionSource {
public:
    CompositePermissionSource(
        const IPermissionSource& first,
        const IPermissionSource& second);

    std::vector<arcs::artifact::ArtifactVersion> load_permission_grants(
        const std::string& principal,
        const std::string& scope) const override;

private:
    const IPermissionSource& first_;
    const IPermissionSource& second_;
};

/**
 * @brief Result of resolving permissions: the effective permission set and
 *        the artifacts that back it.
 */
struct PermissionResolution {
    arcs::reducer::EffectivePermissions permissions;
    std::vector<arcs::artifact::ArtifactVersion> artifacts;
};

/**
 * @brief Computes the effective permissions granted to a principal within a
 *        scope, as of a given time source, based only on permission sources.
 */
class PermissionService {
public:
    /**
     * @brief Resolves effective permissions for a principal within a scope.
     * @param principal Principal to resolve permissions for.
     * @param scope Scope to resolve permissions within.
     * @param time_source Time source used to evaluate time-bounded grants.
     * @param source Permission source used to load backing grant artifacts.
     * @return The resolved permission set and supporting artifacts.
     */
    PermissionResolution resolve_permissions(
        const std::string& principal,
        const std::string& scope,
        const arcs::reducer::ITimeSource& time_source,
        const IPermissionSource& source) const;
};

} // namespace arcs::core::services
