#!/bin/sh
# One-time npm setup for trusted publishing (run it yourself, logged in to
# npm with 2FA, shortly before the first release that publishes to npm).
#
# npm only lets you configure a trusted publisher on a package that already
# exists, and OIDC cannot do a package's first publish. So for each
# @filegoblin package that does not exist yet, this publishes a placeholder
# version (0.0.0-bootstrap, under the dist-tag `bootstrap`, deprecated right
# away), then points every package's trusted publisher at
# .github/workflows/publish.yml with environment `npm`.
#
# A new trusted-publisher configuration expires if no publish uses it within
# two days, so push the release tag soon after running this.
#
# usage: scripts/npm-bootstrap.sh [PARTS]   (PARTS: number of model parts, default 6;
#        `ls npm-packages | grep -c models-part` after package_registries.py)
set -eu

PARTS=${1:-6}
REPO=funkyfunc/filegoblin
WORKFLOW=publish.yml
ENVIRONMENT=npm

NAMES="filegoblin"
for p in darwin-arm64 darwin-x64 linux-x64 linux-arm64 win32-x64; do NAMES="$NAMES @filegoblin/$p"; done
NAMES="$NAMES @filegoblin/models"
i=1
while [ "$i" -le "$PARTS" ]; do NAMES="$NAMES @filegoblin/models-part$i"; i=$((i + 1)); done

npm --version | awk -F. '{ if ($1 < 11 || ($1 == 11 && $2 < 15)) { print "npm >= 11.15.0 is needed for `npm trust` (npm install -g npm@latest)"; exit 1 } }'
npm whoami > /dev/null || { echo "log in first: npm login"; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for name in $NAMES; do
  if npm view "$name" name > /dev/null 2>&1; then
    echo "exists: $name"
    continue
  fi
  dir="$tmp/$(echo "$name" | tr '/@' '__')"
  mkdir -p "$dir"
  cat > "$dir/package.json" <<JSON
{
  "name": "$name",
  "version": "0.0.0-bootstrap",
  "description": "Placeholder that reserves the name for filegoblin's release workflow. Install filegoblin instead.",
  "license": "MIT",
  "repository": { "type": "git", "url": "git+https://github.com/$REPO.git" }
}
JSON
  echo "placeholder: $name"
  npm publish "$dir" --access public --tag bootstrap
  PLACEHOLDERS="${PLACEHOLDERS:-} $name"
done

# A new package takes a few minutes to become visible after publishing
# (npm stages it first, adding its own 0.0.0-stage version), and it cannot be
# deprecated until then; retry for up to ten minutes. `<1.0.0` covers both
# placeholder versions.
for name in ${PLACEHOLDERS:-}; do
  tries=0
  until npm deprecate "$name@<1.0.0" "Placeholder; install filegoblin instead." 2>/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -ge 20 ]; then
      echo "  could not deprecate $name's placeholders yet; run later:"
      echo "  npm deprecate \"$name@<1.0.0\" \"Placeholder; install filegoblin instead.\""
      break
    fi
    echo "  waiting for $name to become visible..."
    sleep 30
  done
done

for name in $NAMES; do
  if npm trust list "$name" 2>/dev/null | grep -q "$WORKFLOW"; then
    echo "trusted already: $name"
    continue
  fi
  echo "trust: $name -> $REPO $WORKFLOW ($ENVIRONMENT)"
  npm trust github "$name" --repo "$REPO" --file "$WORKFLOW" --env "$ENVIRONMENT" --allow-publish --yes ||
    echo "  could not set it (a different trusted publisher already configured? see npm trust list $name)"
done

echo "Done. Push the release tag within two days so the new trusted publishers are used."
