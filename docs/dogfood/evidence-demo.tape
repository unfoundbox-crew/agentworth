# AgentWorth / Evidence, 45-second demo (docs/specs/evidence-check.md section 4.4).
#
# Record with vhs (https://github.com/charmbracelet/vhs):   vhs docs/dogfood/evidence-demo.tape
#
# Not recorded in CI. Before recording, by hand:
#   1. A fixture repository at ~/demo/widget with a GitHub remote you can open PRs on, the
#      Evidence workflow committed (`archie repo receipt init --write`), and the receipt key
#      linked (`archie repo receipt key --github`).
#   2. One real agent session in the index that edited src/lib.rs and ran `cargo test`, and a
#      branch `demo/evidence` with its commits not yet pushed (`archie scan` first).
#   3. After the terminal part, cut to the PR's Checks tab showing the AgentWorth / Evidence
#      table, and highlight the one commit in "Claimed only". That cut is edited in, not taped.
# The receipt id, session count and rung printed below come from the real index; do not
# fake them.

Output docs/dogfood/evidence-demo.gif

Set Shell "bash"
Set FontSize 18
Set Width 1200
Set Height 640
Set Padding 24
Set TypingSpeed 45ms
Set Theme "Builtin Dark"

Hide
Type "cd ~/demo/widget && git switch -q demo/evidence && clear"
Enter
Show

Type "git log --oneline origin/main..HEAD"
Enter
Sleep 2s

# The hook signs and pushes the note: "receipt XXXXXXXX signed, N sessions, rung R attested".
Type "git push -u origin demo/evidence"
Enter
Sleep 6s

Type "archie repo receipt show HEAD"
Enter
Sleep 5s

Type "gh pr create --fill --draft && gh pr checks --watch"
Enter
Sleep 15s
