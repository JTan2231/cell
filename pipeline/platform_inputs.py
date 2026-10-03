"""Explicit CI ownership for installation and shared platform behavior.

Paths are repository-relative fnmatch patterns. Product patterns apply only
inside a descriptor's product root. Keep mixed runtime/lifecycle files explicit
until their lifecycle code has its own module.
"""

PRODUCT_INPUTS = (
    "*/src/bin/*-install.rs", "*/src/bin/*-install/*",
    "*/src/bin/installation/*", "*/src/installation.rs",
    "*/src/maintenance.rs", "*/src/migration*.rs", "*/migrations/*",
    "*/schema.sql", "*/migration*.sql", "*/tests/install.rs",
    "*/tests/maintenance.rs", "*/tests/fixtures/schema*",
    "*/packaging/*", "*/release.sh", "*/ci.sh", "*/Cargo.toml",
)

# Installer-invoked commands, embedded schemas, and service/maintenance entry
# points also belong to the installation boundary, even inside product code.
PRODUCT_RUNTIME_INPUTS = {
    "bazaar": ("infrastructure/bazaar/src/api.rs",),
    "mantic": ("products/mantic/src/store.rs", "products/mantic/src/lib.rs"),
    "conatus": ("products/conatus/src/main.rs", "products/conatus/src/store.rs"),
    "clew": ("products/clew/src/store.rs", "products/clew/src/main.rs", "products/clew/src/lib.rs",
             "products/clew/src/delivery.rs"),
    "annals": ("products/annals/crates/annals/src/db.rs", "products/annals/crates/annals/src/cli.rs",
               "products/annals/crates/annals/src/main.rs", "products/annals/crates/annals/src/sqlite.rs"),
    "nucleus": ("infrastructure/nucleus/crates/nucleus-cli/src/service.rs",
                "infrastructure/nucleus/crates/nucleus-cli/src/main.rs",
                "infrastructure/nucleus/crates/nucleus-store/src/lib.rs",
                "infrastructure/nucleus/crates/nucleus-daemon/src/lib.rs"),
    "decisions": ("products/decisions/crates/decisions/src/store.rs",
                  "products/decisions/crates/decisions/src/cli.rs",
                  "products/decisions/crates/decisions/src/main.rs"),
    "semantics": ("infrastructure/semantics/src/store.rs", "infrastructure/semantics/src/cli.rs",
                  "infrastructure/semantics/src/main.rs"),
    "platter": ("products/platter/src/main.rs", "products/platter/src/cli.rs", "products/platter/src/store.rs",
                "products/platter/src/readiness.rs"),
    "paperboy": ("products/paperboy/src/main.rs", "products/paperboy/src/manifest.rs",
                 "products/paperboy/src/schedule.rs", "products/paperboy/src/lib.rs"),
    "weaver": ("products/weaver-narrative/src/main.rs", "products/weaver-narrative/src/operations.rs",
               "products/weaver-narrative/src/store.rs", "products/weaver-narrative/src/lib.rs",
               "products/weaver-narrative/src/schema.sql", "products/weaver-narrative/src/agent.rs"),
}

# A shared change selects its own suite. Only installation primitives expand
# to consumer installation suites; ordinary shared dependencies do not.
SHARED_INPUTS = {
    "pipeline": ("ci.sh", "ci_manager/*", "pipeline/*.py", "pipeline/*.sh", "pipeline/products/*.sh",
                 "deployment/signing.py", "deployment/test_signing.py"),
    "install": ("deployment/crates/cell-install/*",),
    "maintenance": ("deployment/crates/cell-maintenance/*",),
    "catalog": ("pipeline/integrated.sh", "*/chancery/*.json",
                "*/chancery-*/*.json", "infrastructure/chancery/provider/*.json"),
}

# Bazaar owns prompt resolution and the reviewed seed. Changes to these inputs
# also select the requesters that use the shared prompt interface.
PROMPT_INPUTS = (
    "infrastructure/bazaar/src/prompts.rs",
    "infrastructure/bazaar/src/prompt_import.rs",
    "infrastructure/bazaar/seed.json",
)
PROMPT_CONSUMERS = frozenset((
    "annals", "decisions", "semantics", "platter", "weaver", "emt", "conatus",
))

# cell-install is the common installer for every current product. A newly
# introduced product also gets this suite. cell-maintenance has fewer consumers.
MAINTENANCE_CONSUMERS = frozenset((
    "nucleus", "annals", "decisions", "semantics",
    "platter", "weaver", "clew",
))

PLATFORM_PACKAGES = frozenset(("cell-install", "cell-maintenance"))
PLATFORM_TEST_TARGETS = frozenset(("install", "maintenance"))


def platform_target(package: str, target: dict) -> bool:
    return (package in PLATFORM_PACKAGES
            or ("bin" in target["kind"] and target["name"].endswith("-install"))
            or ("test" in target["kind"] and target["name"] in PLATFORM_TEST_TARGETS))
