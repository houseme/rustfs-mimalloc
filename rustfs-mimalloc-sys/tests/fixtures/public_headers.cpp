#include "mimalloc/qsbr.h"
#include <cerrno>

static int last_error;
extern "C" void on_error(int code, void*) { last_error = code; }

int main() {
  mi_register_error(&on_error, nullptr);
  mi_heap_t* heap = mi_heap_new_qsbr();
  mi_register_error(nullptr, nullptr);
  if (heap != nullptr || last_error != ENOSYS) return 1;
  void* p = mi_malloc(73);
  if (!p) return 2;
  mi_free_sized(p, 73);
  p = mi_malloc_aligned(73, 4096);
  if (!p) return 3;
  mi_free_aligned_sized(p, 4096, 73);
  return 0;
}
