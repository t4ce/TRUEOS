TRUEOS thread/carrier experiment
================================

This directory holds the first source reference and extraction target for a
stackful thread/carrier side path beside the Embassy AP executor.

`trustos/` contains Apache-2.0 licensed source imported from:

https://github.com/nathan237/TrustOS

Imported commit:

`4eaace7eeb65c665b87c69c5db74c9dcf5073f11`

`trustos/` is the raw upstream snapshot. Keep it close to the imported source
so provenance stays clear.

`trust_thread/` is the cleaned extraction target. It starts pushing TrustOS
thread semantics behind a small portable boundary: saved contexts, run queues,
thread/carrier vocabulary, and a platform hook trait.

Neither tree is wired into `src/main.rs` yet. The goal is to make the thread
substrate real enough to compile on its own before it enters TRUEOS' AP loop.
