# Live LogOs controls

Run `log` from Shell3's System mode. It enters the same native terminal lease
as the other System tools, locally or over SSH. The first three rows use the
Shell3 control background; the body uses the default terminal background.

The area table shows the live level and policy. Enter or click an area to edit
it. Up includes the chosen level and every level toward Error; Down includes
the chosen level and every level toward Trace. Only lets you toggle an exact
set. All and None are available for one area or all areas. Arrow keys or j/k
select, Enter chooses, mouse clicks choose, and the wheel moves selection.
Esc/q/Left returns to the previous page, or leaves the tool from the area table.

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
explicit policy or profile application starts the §log transcript and live
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
