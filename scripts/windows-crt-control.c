// Independent of Rust and mimalloc: verify native MSVC /MT startup.
#include <stdlib.h>
#include <stdio.h>
int main(void) {
  void* bytes = malloc(64 * 1024);
  if (!bytes) return 1;
  free(bytes);
  puts("Native C static CRT control passed");
  return 0;
}
