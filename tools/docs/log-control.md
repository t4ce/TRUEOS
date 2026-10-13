# Live LogOs controls

Run `log` from Shell3's System mode. It enters the same native terminal lease
as the other System tools, locally or over SSH. The first three rows use the
Shell3 control background; the body uses the default terminal background.

The main screen places each area name above its checkbox row, followed by a
blank line. Checkboxes appear in presentation order: none, once, user, ai,
error, important, warn, info, debug, trace. This order does not change severity
ordering. Profiles, Live log stream, and Set all areas appear below the areas.

Arrow keys move up/down between areas and left/right between checkboxes; WASD
mirrors these keys. Enter or Space toggles the focused box, which is underlined
in pink. Only the checkbox glyph accepts a mouse toggle: labels, spacing, area
names and headers do nothing when clicked. Action labels choose their pages.
The wheel moves up/down. Narrow terminals wrap checkbox rows; the viewport
scrolls to keep focus visible. Esc/q goes back or leaves the main grid.

Build Up/Down policies are shown as their effective checked levels. Browsing
preserves them; changing a checkbox replaces that area's policy with an exact
set while preserving every other enabled level. None clears the area. Set all
areas offers the same grid plus All levels: a level toggle changes only that
level across all areas, preserving each area's other selections. Its checkbox
is checked when every area admits that level; None is checked when every area
is empty.

Mouse hover gives only the checkbox a slightly lighter background. Hovering
any letter of a level word reveals `up` above it and `down` below, aligned to
the word's first column. None has no range actions. Moving onto either action
keeps it visible; moving onto a checkbox, a gap, or elsewhere hides both.
One click consumes the actions: up selects the hovered level and every level
to its left in the displayed row, while down selects it and every level to its
right. All other levels are cleared. These are exact sets in presentation
order, independent of the build's severity thresholds. The same actions in
Set all areas replace every area's selection. Hovering does not start capture.

Policies start from the effective compile-time profile matrix, including its
override precedence. Edits immediately affect the common acceptance gate used
by TCP and emulator UART logging. They last until reboot. Build defaults reapplies
the compiled matrix; there is no disk persistence or saved policy snapshot.

The shared LogOs crate and its older vendor copy generate record levels,
filter levels, level sets, names, and ordered lists from one declaration:
Error, User, Ai, Important, Warn, Once, Info, Debug, Trace. User and Ai are
available types; existing callsites retain their levels. Off is only a filter
state and cannot be emitted as a record. The level set uses u16 for nine bits.

Profiles selects standalone filter matrices for Normal, TGL GPU, Archive,
Blueprint/HV, Font warm, USB runtime, USB full/UAS, EarlyBoot, HTTP fetch,
Wi-Fi PCI, Lumen performance, UI4 planes, Shell2 render, QuadTexture, and
Helio graphics. Each replaces all area filters; manual edits mark the profile
Custom. EarlyBoot opens Boot/Trace plus Global, Net, Service, USB and Storage/Info.
USB full/UAS opens USB and Global/Trace plus Boot and Storage/Info. Unrelated
areas remain at Warn in focused profiles. The matrices live in
`src/log_policy.rs` and can be extended ahead of a build.

Profile selection changes acceptance, not compiled probe instrumentation.
Optional UAS/HID, font, Lumen, Shell2 and QuadTexture probes still depend on
their build switches. EarlyBoot affects future records and cannot recover
already filtered boot records. Once suppression and rate limiting also remain
in effect.

Opening the editor or browsing its pages leaves collection idle. The first
actual checkbox change or explicit profile application starts the §log transcript and live
reader. Collection continues when the editor is parked; freeing the slot stops
it. Active background collection shows the filled spinner rotating in reverse
after §log. This activity is separate from slot selection and VMX affinity,
whose existing sparse spinner is unchanged.

Live log stream shows accepted records from an independent bounded reader of
the log ring. Up/wheel scrolls retained history; Down moves toward the live end.
The existing §log transcript continues to collect independently, and neither
display consumes TCP's capture stream. Display readers report overwritten
bytes instead of splicing partial records.

Validation: `python3 tools/testpy/test_live_log_policy.py` compiles production
policy, TCP/UART adapters, terminal input, renderer and stream code for the host.
The shared crate's unit tests cover level ordering, masks and routing.
