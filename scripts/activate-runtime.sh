#!/usr/bin/env bash

# The caller validates a complete staging directory before publishing it.
# Sibling renames stay on one filesystem; a failed publish restores the old tree.
activate_runtime() {
  local staged="$1" destination="$2" backup
  if [[ "$(dirname "$staged")" != "$(dirname "$destination")" || ! -d "$staged" ]]; then
    echo "Runtime staging must be a sibling of its destination." >&2
    return 1
  fi
  backup="$(mktemp -d "${destination}.backup.XXXXXX")" || return 1
  rmdir "$backup" || return 1
  if [[ -e "$destination" ]]; then
    mv "$destination" "$backup" || return 1
  fi
  if ! mv "$staged" "$destination"; then
    if [[ -d "$backup" ]] && ! mv "$backup" "$destination"; then
      echo "Runtime restore failed; previous runtime remains at $backup" >&2
    fi
    return 1
  fi
  if [[ -d "$backup" ]] && ! rm -rf "$backup"; then
    echo "Runtime active; old backup could not be removed: $backup" >&2
  fi
}
