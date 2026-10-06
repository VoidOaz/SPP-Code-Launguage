/* SPP native core — C11.
 *
 * Low-level primitives that benefit from tight, allocation-free C code:
 *   - FNV-1a 64-bit string hashing (used by the VM's constant interning and
 *     future module caches), processed in an unrolled 8-bytes-per-iteration
 *     loop for throughput on long identifiers/strings.
 *   - A public version stamp so tooling can detect which native ABI is linked
 *     into the running `spp` binary.
 *
 * The C objects are compiled together with the C++ objects into a single
 * static archive (libspp_native.a / .lib) and consumed from Rust via FFI in
 * src/native.rs.
 */
#include "spp_native.h"

#define SPP_NATIVE_VERSION 0x00020100u /* v2.1.0 encoded as 0xMMmmppbb */

static const uint64_t SPP_FNV_OFFSET = 0xcbf29ce484222325ULL;
static const uint64_t SPP_FNV_PRIME  = 0x100000001b3ULL;

uint64_t spp_fnv1a64(const uint8_t *data, uint64_t len) {
    uint64_t h = SPP_FNV_OFFSET;
    if (data == 0) return h;

    /* Unrolled main loop: 8 bytes per iteration keeps the multiplier chain
     * fed while minimizing branch overhead on identifier-sized inputs. */
    uint64_t i = 0;
    const uint64_t n8 = len & ~7ULL;
    for (; i < n8; i += 8) {
        h ^= data[i + 0]; h *= SPP_FNV_PRIME;
        h ^= data[i + 1]; h *= SPP_FNV_PRIME;
        h ^= data[i + 2]; h *= SPP_FNV_PRIME;
        h ^= data[i + 3]; h *= SPP_FNV_PRIME;
        h ^= data[i + 4]; h *= SPP_FNV_PRIME;
        h ^= data[i + 5]; h *= SPP_FNV_PRIME;
        h ^= data[i + 6]; h *= SPP_FNV_PRIME;
        h ^= data[i + 7]; h *= SPP_FNV_PRIME;
    }
    for (; i < len; ++i) {
        h ^= data[i];
        h *= SPP_FNV_PRIME;
    }
    return h;
}

uint32_t spp_native_version(void) {
    return SPP_NATIVE_VERSION;
}
