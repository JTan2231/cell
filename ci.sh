#!/bin/sh

set -eu

ROOT=$(CDPATH='' cd "$(dirname "$0")" && pwd)

usage() {
    printf '%s\n' \
        'Usage: ./ci.sh [nucleus|annals|todo|chancery|weaver|email|conversations|decisions|semantics|geste|pratica|clockwork|crm]...'
}

if [ "$#" -eq 0 ]; then
    set -- nucleus annals todo chancery weaver email conversations decisions semantics geste pratica clockwork crm
fi

nucleus_selected=0
annals_selected=0
todo_selected=0
chancery_selected=0
weaver_selected=0
email_selected=0
conversations_selected=0
decisions_selected=0
semantics_selected=0
geste_selected=0
pratica_selected=0
clockwork_selected=0
crm_selected=0
for project in "$@"; do
    case "$project" in
        nucleus) nucleus_selected=1 ;;
        annals) annals_selected=1 ;;
        todo) todo_selected=1 ;;
        chancery) chancery_selected=1 ;;
        weaver) weaver_selected=1 ;;
        email) email_selected=1 ;;
        conversations) conversations_selected=1 ;;
        decisions) decisions_selected=1 ;;
        semantics) semantics_selected=1 ;;
        geste) geste_selected=1 ;;
        pratica) pratica_selected=1 ;;
        clockwork) clockwork_selected=1 ;;
        crm) crm_selected=1 ;;
        *)
            usage >&2
            exit 2
            ;;
    esac
done

for project in "$@"; do
    printf '==> %s CI\n' "$project"
    "$ROOT/$project/ci.sh"
done

if [ "$nucleus_selected$annals_selected$todo_selected$chancery_selected$weaver_selected$email_selected$conversations_selected$decisions_selected$semantics_selected$geste_selected$pratica_selected$clockwork_selected$crm_selected" = \
    1111111111111 ]
then
    printf '%s\n' '==> integrated Chancery source catalog'
    (
        catalog_workspace=$(mktemp -d "${TMPDIR:-/tmp}/cell-catalog.XXXXXX")
        catalog_workspace=$(CDPATH='' cd "$catalog_workspace" && pwd)
        catalog_registry="$catalog_workspace/providers"
        mkdir "$catalog_registry"
        cleanup_catalog_registry() {
            rm -rf "$catalog_workspace"
        }
        trap cleanup_catalog_registry EXIT
        trap 'exit 1' HUP INT TERM

        ln -s "$ROOT/chancery/provider" "$catalog_registry/chancery"
        ln -s "$ROOT/nucleus/chancery" "$catalog_registry/nucleus"
        ln -s "$ROOT/annals/chancery/annals" "$catalog_registry/annals"
        ln -s "$ROOT/annals/chancery/annals-usage" \
            "$catalog_registry/annals-usage"
        ln -s "$ROOT/todo/chancery" "$catalog_registry/todo"
        ln -s "$ROOT/weaver/chancery" "$catalog_registry/weaver"
        ln -s "$ROOT/email/chancery" "$catalog_registry/email"
        ln -s "$ROOT/conversations/chancery" "$catalog_registry/conversations"
        ln -s "$ROOT/decisions/chancery" "$catalog_registry/decisions"
        ln -s "$ROOT/semantics/chancery" "$catalog_registry/semantics"
        ln -s "$ROOT/geste/chancery" "$catalog_registry/geste"
        ln -s "$ROOT/pratica/chancery" "$catalog_registry/pratica"
        ln -s "$ROOT/clockwork/chancery" "$catalog_registry/clockwork"
        ln -s "$ROOT/crm/chancery" "$catalog_registry/crm"

        chancery_candidate="$ROOT/target/release/chancery"
        [ -f "$chancery_candidate" ] && [ -x "$chancery_candidate" ] || {
            printf 'ci.sh: Chancery release candidate is unavailable: %s\n' \
                "$chancery_candidate" >&2
            exit 1
        }
        "$chancery_candidate" --registry "$catalog_registry" doctor
        "$chancery_candidate" --registry "$catalog_registry" --json list \
            >/dev/null

        normalized_entries=0
        for provider_path in "$catalog_registry"/*; do
            provider_id=${provider_path##*/}
            grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*3' \
                "$provider_path/provider.json" || {
                printf 'ci.sh: provider is not schema 3: %s\n' "$provider_id" >&2
                exit 1
            }
            grep -F '"promise_scope"' "$provider_path/provider.json" \
                >/dev/null || {
                printf 'ci.sh: provider has no promise scope: %s\n' \
                    "$provider_id" >&2
                exit 1
            }
            for entry_path in "$provider_path"/entries/*.json; do
                grep -F '"promise"' "$entry_path" >/dev/null || {
                    printf 'ci.sh: entry has no normalized promise: %s\n' \
                        "$entry_path" >&2
                    exit 1
                }
                entry_id=$(awk -F '"' '
                    /^[[:space:]]*"id"[[:space:]]*:/ { print $4; exit }
                ' "$entry_path")
                [ -n "$entry_id" ] || {
                    printf 'ci.sh: entry has no readable ID: %s\n' \
                        "$entry_path" >&2
                    exit 1
                }
                resolution="$catalog_workspace/resolution-$normalized_entries.json"
                set +e
                "$chancery_candidate" --registry "$catalog_registry" \
                    --json resolve "$entry_id" >"$resolution"
                resolution_status=$?
                set -e
                [ "$resolution_status" -le 1 ] || {
                    printf 'ci.sh: entry resolution failed structurally: %s\n' \
                        "$entry_id" >&2
                    exit 1
                }
                if grep -Eq '"code":"(provider_scope_undeclared|provider_inventory_partial|facet_undeclared)"' \
                    "$resolution"
                then
                    printf 'ci.sh: entry has undeclared promise coverage: %s\n' \
                        "$entry_id" >&2
                    exit 1
                fi
                grep -F '"dependency_closure_status":"complete"' \
                    "$resolution" >/dev/null || {
                    printf 'ci.sh: entry dependency closure is incomplete: %s\n' \
                        "$entry_id" >&2
                    exit 1
                }
                grep -F '"issues":[]' "$resolution" >/dev/null || {
                    printf 'ci.sh: entry resolution has catalog issues: %s\n' \
                        "$entry_id" >&2
                    exit 1
                }
                normalized_entries=$((normalized_entries + 1))
            done
        done
        [ "$normalized_entries" -eq 51 ] || {
            printf 'ci.sh: expected 51 normalized entries; found %s\n' \
                "$normalized_entries" >&2
            exit 1
        }

        "$chancery_candidate" --registry "$catalog_registry" --json resolve \
            decisions.lifecycle.consume \
            --require completeness_and_freshness \
            >/dev/null

        annals_resolution="$catalog_workspace/annals-usage-resolution.json"
        set +e
        "$chancery_candidate" --registry "$catalog_registry" --json resolve \
            annals-usage.consumption.inspect >"$annals_resolution"
        annals_resolution_status=$?
        set -e
        [ "$annals_resolution_status" -eq 1 ] || {
            printf 'ci.sh: Annals Usage resolution should report incomplete declaration; exit %s\n' \
                "$annals_resolution_status" >&2
            exit 1
        }
        grep -F '"status":"incomplete_declaration"' "$annals_resolution" \
            >/dev/null
        grep -F '"code":"uncontracted_reliance"' "$annals_resolution" \
            >/dev/null
    )
fi

printf '%s\n' 'ci.sh: all selected project gates are green'
