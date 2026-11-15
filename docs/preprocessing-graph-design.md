# M0 custom-command designs (before prototype)

The production driver and CMake modules are not changed by this investigation.
The following candidates will be compared with real generator output, not inferred
from documentation. The existing native OBJECT_OUTPUTS probe remains a negative
control, including its Make success and Ninja failures.

| Design | Expected Ninja behavior | Make concern |
| --- | --- | --- |
| completion OUTPUT, payload BYPRODUCTS | Ninja knows all outputs | missing byproducts may have no recovery recipe |
| completion and every required file OUTPUT | real ownership of every file | secondary-output checking/touching may affect timestamps |

Prototype the second design only if the comparison confirms recovery. Every
target/configuration gets a separate output directory and a single writer. A
command-bearing preprocessing edge is reached through a command-free ordering
target. Native compilation has both an ordering dependency and file dependencies
on generated payload and snapshot metadata. Its source is still an ordinary Rpp
crate; a test-only adapter selects the generated root and checks completion state.

The completion stamp is a scheduling output, touched after every successful
generation so unchanged content cannot leave the command perpetually dirty.
Payload/map/manifest writes compare bytes first. Compilation depends on content
outputs, not just the scheduling stamp. The fixture uses no Rust preprocessing
syntax: it copies sources/assets and inserts one numeric fixture value.

The preprocessing depfile is a required OUTPUT for Make only, analogous to the
existing compiler-depfile co-output fix. For Ninja, measure depfile/dependency-log
behavior: an already consumed depfile must not become a permanently missing
required output. Test editing dependencies with and without on-disk depfiles.

Before updating M0 status, test interruption after completion invalidation,
snapshot rejection at compile and final-link entry, timestamp behavior, genuine
zero-command no-ops, installed-only consumers, and target/configuration ownership.
No conclusion or release-gate relaxation follows from this design note alone.
