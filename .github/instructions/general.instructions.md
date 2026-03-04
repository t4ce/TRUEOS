---
description: Afer code changes
applyTo: '**'
---
cargo fmt
timeout 125 make run

afterwards you can see in terminal the logs

A lot of times you tend to keep questionable fallbacks after the actual path is already advanced.
that causes obvouscation and is often leaving basically dead code - where we could easily save some loc instead. therefore after we tinkered with something that was unclear first, try work with this in mind.