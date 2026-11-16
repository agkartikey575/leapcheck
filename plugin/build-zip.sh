#!/usr/bin/env bash
# Build the WordPress.org submission zip: leapcheck-<version>.zip
# Contains only the leapcheck/ folder (no tests, no build script, no dotfiles).
# Usage: plugin/build-zip.sh [output-dir]   (default: the plugin/ folder)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src="$here/leapcheck"
out_dir="${1:-$here}"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"

command -v zip >/dev/null || { echo "error: 'zip' is not installed" >&2; exit 1; }

# The version in the plugin header, the LEAPCHECK_VERSION constant and the readme Stable tag must agree.
version="$(sed -n 's/^ \* Version:[[:space:]]*\([^[:space:]]*\).*/\1/p' "$src/leapcheck.php" | head -n1)"
constant="$(sed -n "s/^define( 'LEAPCHECK_VERSION', '\([^']*\)' );/\1/p" "$src/leapcheck.php" | head -n1)"
stable="$(sed -n 's/^Stable tag:[[:space:]]*\([^[:space:]]*\).*/\1/p' "$src/readme.txt" | head -n1)"
if [[ -z "$version" || "$version" != "$constant" || "$version" != "$stable" ]]; then
	echo "error: version mismatch: header='$version' constant='$constant' readme Stable tag='$stable'" >&2
	exit 1
fi
if ! grep -q "^= $version =" "$src/readme.txt"; then
	echo "error: readme.txt has no changelog entry '= $version ='" >&2
	exit 1
fi

# Syntax check every PHP file when PHP is available.
if command -v php >/dev/null; then
	while IFS= read -r -d '' f; do
		php -l "$f" >/dev/null || { echo "error: syntax error in $f" >&2; exit 1; }
	done < <(find "$src" -name '*.php' -print0)
fi

zip_path="$out_dir/leapcheck-$version.zip"
rm -f "$zip_path"
(
	cd "$here"
	zip -r -X -q "$zip_path" leapcheck \
		-x 'leapcheck/.*' 'leapcheck/**/.*' \
		-x '*.DS_Store' '*Thumbs.db' '*.zip' '*.log' '*.orig' '*.swp' \
		-x 'leapcheck/tests/*' 'leapcheck/build-zip.sh' 'leapcheck/node_modules/*' 'leapcheck/vendor/*'
)

echo "Built $zip_path"
unzip -l "$zip_path"
