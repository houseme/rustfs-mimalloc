#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdatomic.h>
#include <stddef.h>
#include <string.h>

typedef struct { bool weak; unsigned char seed; } mi_random_ctx_t;
static bool preloading = true, os_ok = true;
static int os_calls, warnings;
static bool _mi_preloading(void) { return preloading; }
static bool _mi_prim_random_buf(void* key, size_t size) {
  os_calls++;
  memset(key, 0x5a, size);
  return os_ok;
}
static void _mi_warning_message(const char* message) { (void)message; warnings++; }
static size_t mi_random_weak(void) { return 0x1234; }
static size_t mi_random_shuffle(size_t x) { return x; }
static void chacha_init(mi_random_ctx_t* ctx, const uint8_t* key, uintptr_t nonce) {
  (void)nonce; ctx->seed = key[0];
}
#define mi_memzero(p,n) memset(p,0,n)
#define mi_addr(p) ((uintptr_t)(p))
#define mi_atomic_exchange_relaxed(p,v) atomic_exchange_explicit(p,v,memory_order_relaxed)

/* @INIT@ */

int main(void) {
  mi_random_ctx_t ctx = {false, 0};
  _mi_random_init(&ctx);
  assert(ctx.weak && os_calls == 0 && warnings == 0);
  preloading = false;
  _mi_random_init(&ctx);
  assert(!ctx.weak && ctx.seed == 0x5a && os_calls == 1 && warnings == 0);
  os_ok = false;
  _mi_random_init(&ctx);
  _mi_random_init(&ctx);
  assert(ctx.weak && os_calls == 3 && warnings == 1);
  return 0;
}
