#!/bin/sh
# Check the Java that needs no Android (the decisions in the *Logic.java files) on this
# machine: compiled and run with the JDK alone, no test library.
#   android/test-java.sh
# Needs: JAVA_HOME (default: Android Studio's JDK).
set -e
here=$(cd "$(dirname "$0")" && pwd)
export JAVA_HOME="${JAVA_HOME:-/Applications/Android Studio.app/Contents/jbr/Contents/Home}"
package=io/github/aminuddinihsan20/bitchord
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
"$JAVA_HOME/bin/javac" -Xlint:all -Werror -d "$out" \
    "$here"/app/src/main/java/$package/*Logic.java \
    "$here"/app/src/test/java/$package/*LogicTest.java
failed=0
for test in "$here"/app/src/test/java/$package/*LogicTest.java; do
    name=$(basename "$test" .java)
    printf '%s: ' "$name"
    "$JAVA_HOME/bin/java" -cp "$out" "io.github.aminuddinihsan20.bitchord.$name" || failed=1
done
exit $failed
