# set_ini FILE SECTION KEY VALUE
#
# Sets KEY=VALUE in [SECTION] of an INI file (/etc/wsl.conf, .wslconfig) and
# keeps every other line as it was: other keys, other sections, comments,
# blank lines and CRLF line ends. A missing key is added at the end of its
# section, a missing section at the end of the file. Before the first change
# the original is kept as FILE.before-dockernanny; a later change never
# overwrites that copy. Returns 0 when the file changed, 1 when it already
# said so. Names match without regard to case, as WSL reads them.
set_ini() {
  ini_file=$1
  ini_section=$2
  ini_key=$3
  ini_value=$4
  if [ -f "$ini_file" ] && [ "$(od -An -tx1 -N2 "$ini_file" | tr -d ' \n')" = "fffe" ]; then
    echo "$ini_file is saved as UTF-16; left as it is. Add $ini_key=$ini_value under [$ini_section] yourself." >&2
    return 1
  fi
  [ -f "$ini_file" ] || : > "$ini_file"
  awk -v section="$ini_section" -v key="$ini_key" -v value="$ini_value" '
    function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t\r]+$/, "", s); return s }
    function flush() { printf "%s", held; held = "" }
    BEGIN { wanted = "[" tolower(section) "]"; inside = 0; done = 0; held = ""; eol = "" }
    {
      line = $0
      if (line ~ /\r$/) { eol = "\r" }
      text = trim(line)
      if (text ~ /^\[.*\]$/) {
        if (inside && !done) { print key "=" value eol; done = 1 }
        flush()
        inside = (tolower(text) == wanted)
        print line
        next
      }
      if (inside && text == "") { held = held line "\n"; next }
      if (inside && !done) {
        at = index(text, "=")
        if (at > 0 && tolower(trim(substr(text, 1, at - 1))) == tolower(key)) {
          flush()
          print key "=" value eol
          done = 1
          next
        }
      }
      flush()
      print line
    }
    END {
      if (!done && inside) { print key "=" value eol; done = 1 }
      flush()
      if (!done) {
        if (NR > 0) { print eol }
        print "[" section "]" eol
        print key "=" value eol
      }
    }' "$ini_file" > "$ini_file.dn-new" || { rm -f "$ini_file.dn-new"; return 1; }
  if cmp -s "$ini_file" "$ini_file.dn-new"; then
    rm -f "$ini_file.dn-new"
    return 1
  fi
  if [ -s "$ini_file" ] && [ ! -e "$ini_file.before-dockernanny" ]; then
    cp "$ini_file" "$ini_file.before-dockernanny"
  fi
  cat "$ini_file.dn-new" > "$ini_file"
  rm -f "$ini_file.dn-new"
  return 0
}

# ini_missing FILE SECTION KEY: true when KEY has no value under [SECTION].
ini_missing() {
  [ -f "$1" ] || return 0
  awk -v section="$2" -v key="$3" '
    function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t\r]+$/, "", s); return s }
    BEGIN { wanted = "[" tolower(section) "]"; inside = 0; found = 0 }
    {
      text = trim($0)
      if (text ~ /^\[.*\]$/) { inside = (tolower(text) == wanted); next }
      at = index(text, "=")
      if (inside && at > 0 && tolower(trim(substr(text, 1, at - 1))) == tolower(key)) { found = 1 }
    }
    END { exit found ? 1 : 0 }' "$1"
}
