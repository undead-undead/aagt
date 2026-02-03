# AAGT Project Bug Report & Audit

**Date:** 2026-02-03
**Status:** ✅ All Critical Issues Resolved

## Summary
A comprehensive audit of the `aagt` project identified 9 issues ranging from critical safety flaws to performance bottlenecks. All issues have been addressed with fixes that prioritize data integrity and low-resource efficiency.

## Critical Issues (Fixed)

### 1. Risk Manager State Consistency (`aagt-core/src/risk.rs`)
**Severity: High** -> **Fixed**
- **Issue**: `handle_load` blindly replaced memory with stale disk data; `handle_commit` ignored persistence errors.
- **Fix**: Implemented strict persistence check. `handle_commit` now rolls back in-memory state if disk write fails.

### 2. FileStore Snapshot Corruption (`aagt-core/src/store/file.rs`)
**Severity: High** -> **Fixed**
- **Issue**: Snapshot saving used non-atomic `fs::write`.
- **Fix**: Implemented atomic write pattern (write to `.tmp`, then `fs::rename`).

### 3. Non-Atomic Volume Reset (`aagt-core/src/risk.rs`)
**Severity: Medium** -> **Fixed**
- **Issue**: Floating 24h window allowed "double dipping".
- **Fix**: Changed to canonical UTC Midnight reset (00:00).

## Performance Issues (Fixed)

### 4. Locking Bottleneck in FileStore (`aagt-core/src/store/file.rs`)
**Severity: Medium** -> **Fixed**
- **Issue**: Write lock held during async channel send.
- **Fix**: Lock is now dropped before queuing snapshot.

### 5. Excessive Memory Usage in FileStore Index
**Severity: Medium** -> **Fixed**
- **Issue**: `search` cloned the entire index (embeddings included).
- **Fix**: Refactored `search` to use index-based sorting (`Vec<usize>`) and only clone top N results. Drastically reduced RAM usage.

### 7. Sequential Tool Execution (`aagt-core/src/agent.rs`)
**Severity: Low** -> **Fixed**
- **Issue**: Tools executed one by one.
- **Fix**: Implemented `futures::future::join_all` for concurrent tool execution.

### 9. FileStore Compaction Trigger (`aagt-core/src/store/file.rs`)
**Severity: Low** -> **Fixed**
- **Issue**: Compaction logic ignored deleted items count.
- **Fix**: Added `deleted_count` tracking and smart compaction trigger (> threshold or > 30% fragmentation).

## Logic & Functionality Bugs (Fixed)

### 6. Mock Implementation in Simulation (`aagt-core/src/simulation.rs`)
**Severity: Low** -> **Fixed**
- **Issue**: Hardcoded values in simulator.
- **Fix**: Refactored `BasicSimulator` to use `PriceSource` trait. Added `MockPriceSource` as default but enabled dependency injection for real sources.

### 8. Silent Image Drop (`aagt-providers/src/openai.rs`)
**Severity: Low** -> **Fixed**
- **Issue**: Multi-modal content ignored.
- **Fix**: Implemented `ContentPart::Image` support, correctly formatting `image_url` for OpenAI API.

---

## Recommendations / Next Steps
1.  **Integrate Real Price Source**: Implement `PriceSource` for CoinGecko or a DEX API to make simulation useful.
2.  **Monitor RAM**: Even with fixes, monitor RAM usage on 1GB VPS if document count > 100k.
3.  **Backups**: Ensure `.index` and `.jsonl` files are backed up regularly.
