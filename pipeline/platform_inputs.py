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
    "annals": ("annals/crates/annals/src/db.rs", "annals/crates/annals/src/cli.rs",
               "annals/crates/annals/src/main.rs"),
    "nucleus": ("nucleus/crates/nucleus-cli/src/service.rs",
                "nucleus/crates/nucleus-cli/src/main.rs",
                "nucleus/crates/nucleus-store/src/lib.rs",
                "nucleus/crates/nucleus-daemon/src/lib.rs"),
    "todo": ("todo/crates/todo/src/todo_store.rs", "todo/crates/todo/src/cli.rs",
             "todo/crates/todo/src/main.rs"),
    "decisions": ("decisions/crates/decisions/src/store.rs",
                  "decisions/crates/decisions/src/cli.rs",
                  "decisions/crates/decisions/src/main.rs"),
    "semantics": ("semantics/src/store.rs", "semantics/src/cli.rs",
                  "semantics/src/main.rs"),
    "crm": ("crm/crates/crm/src/store.rs", "crm/crates/crm/src/cli.rs",
            "crm/crates/crm/src/main.rs"),
    "platter": ("platter/src/main.rs", "platter/src/cli.rs", "platter/src/store.rs",
                "platter/src/readiness.rs"),
    "paperboy": ("paperboy/src/main.rs", "paperboy/src/operations.rs",
                 "paperboy/src/store.rs", "paperboy/src/lib.rs"),
}

# A shared change selects its own suite. Only installation primitives expand
# to consumer installation suites; ordinary shared dependencies do not.
SHARED_INPUTS = {
    "pipeline": ("ci.sh", "pipeline/*.py", "pipeline/*.sh", "pipeline/products/*.sh"),
    "broker": ("ci_broker/*.py", "ci_broker/*.sh"),
    "deployment": ("deploy.sh", "deployment/cli.py",
                   "deployment/candidate.py", "deployment/__init__.py",
                   "deployment/test_coordinator.py"),
    "build": ("Cargo.toml", "rust-toolchain.toml", ".cargo/*",
              "deployment/build.py", "deployment/candidate.py", "deployment/__init__.py",
              "deployment/test_build.py"),
    "cleanup": ("deployment/cleanup.py", "deployment/__init__.py", "deployment/test_cleanup.py"),
    "install": ("deployment/crates/cell-install/*", "deployment/tests/simple_fixture.rs"),
    "maintenance": ("deployment/crates/cell-maintenance/*",),
    "catalog": ("pipeline/integrated.sh", "pipeline/extras/*", "*/chancery/*.json",
                "*/chancery-*/*.json", "chancery/provider/*.json"),
}

# cell-install is the common installer for every current product. A newly
# introduced product also gets this suite. cell-maintenance has fewer consumers.
MAINTENANCE_CONSUMERS = frozenset((
    "nucleus", "annals", "todo", "decisions", "semantics", "crm",
    "platter", "paperboy",
))
INSTALL_FIXTURE_CONSUMERS = frozenset((
    "cast", "clockwork", "chancery", "email", "crm", "conversations",
))

PLATFORM_PACKAGES = frozenset(("cell-install", "cell-maintenance"))
PLATFORM_TEST_TARGETS = frozenset(("install", "maintenance"))


def platform_target(package: str, target: dict) -> bool:
    return (package in PLATFORM_PACKAGES
            or ("bin" in target["kind"] and target["name"].endswith("-install"))
            or ("test" in target["kind"] and target["name"] in PLATFORM_TEST_TARGETS))
