/* Controlled host-C probe of the pinned parser, PMD detector and mmap advice.
   Source placeholders are filled from the vendored implementation by the test. */
#define _DEFAULT_SOURCE
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <limits.h>
#include <stdio.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>

/* Enable the Linux source branches after including host platform headers. */
#ifndef __linux__
#define __linux__ 1
#endif
#undef MADV_HUGEPAGE
#undef MADV_NOHUGEPAGE
#define MADV_HUGEPAGE 42
#ifdef TEST_HAS_NOHUGEPAGE
#define MADV_NOHUGEPAGE 43
#endif
#define MI_KiB ((size_t)1024)
#define MI_MiB (MI_KiB * 1024)
#define MI_UNIX_MAX_THP_PAGE_SIZE (8 * MI_MiB)
#define MI_OS_HAS_HUGE_PAGES 0
#define MI_UNUSED(x) ((void)(x))
#define mi_option_allow_thp 1

static const char* pmd_text;
static size_t detected_size;
static bool thp_available;
static bool eligible = true;
static bool allocation_succeeds = true;
static bool option_enabled = true;
static int advice;
static char region;
static int mi_prim_open(const char* path, int flags) {
  (void)flags;
  assert(strcmp(path, "/sys/kernel/mm/transparent_hugepage/hpage_pmd_size") == 0);
  return pmd_text == NULL ? -1 : 1;
}
static ssize_t mi_prim_read(int fd, void* out, size_t capacity) {
  assert(fd == 1);
  size_t size = strlen(pmd_text);
  if (size > capacity) size = capacity;
  memcpy(out, pmd_text, size);
  return (ssize_t)size;
}
static int mi_prim_close(int fd) { assert(fd == 1); return 0; }
static bool mi_option_is_enabled(int option) { assert(option == mi_option_allow_thp); return option_enabled; }
static int unix_mmap_fd(void) { return -1; }
static bool _mi_os_has_overcommit(void) { return true; }
static bool _mi_os_canuse_large_page(size_t size, size_t alignment) {
  (void)size; (void)alignment; return eligible;
}
static size_t _mi_os_large_page_size(void) { return detected_size; }
static bool _mi_os_has_thp(void) { return thp_available; }
static void* unix_mmap_prim_aligned(void* addr, size_t size, size_t alignment, int protect, int flags, int fd) {
  (void)addr; (void)size; (void)alignment; (void)protect; (void)flags; (void)fd;
  return allocation_succeeds ? &region : NULL;
}
static int unix_madvise(void* p, size_t size, int value) {
  (void)size; assert(p == &region); advice = value; return 0;
}

/* @PARSER@ */
/* @DETECTOR@ */
/* @MMAP@ */

static void check_parser(const char* input, bool valid, long expected) {
  const size_t page = (size_t)sysconf(_SC_PAGESIZE);
  void* mapping = mmap(NULL, page * 2, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
  assert(mapping != MAP_FAILED);
  assert(mprotect((char*)mapping + page, page, PROT_NONE) == 0);
  const size_t length = strlen(input);
  char* start = (char*)mapping + page - length;
  memcpy(start, input, length); /* Deliberately no NUL before the guard page. */
  long value = 99;
  const char* end = NULL;
  assert(_mi_strntol(start, length, &end, &value) == valid);
  if (valid) { assert(value == expected); assert(end == start + length); }
  assert(munmap(mapping, page * 2) == 0);
}

static void check_advice(size_t page_size, size_t mapping_size, bool allow, bool valid_mapping, bool valid_range, int expected) {
  detected_size = page_size;
  thp_available = page_size <= MI_UNIX_MAX_THP_PAGE_SIZE;
  allocation_succeeds = valid_mapping;
  eligible = valid_range;
  advice = 0;
  bool is_large = true;
  void* p = unix_mmap(NULL, mapping_size, MI_MiB, PROT_READ | PROT_WRITE, false, allow, &is_large);
  assert((p != NULL) == valid_mapping);
  assert(!is_large);
  assert(advice == expected);
}

int main(void) {
  check_parser("", false, 0);
  check_parser("+", false, 0);
  check_parser("-", false, 0);
  check_parser("0x", false, 0);
  check_parser("0", true, 0);
  check_parser("42", true, 42);
  check_parser(" \t-42", true, -42);
  check_parser("0X002a", true, 42);
  check_parser("00", false, 0);
  check_parser("invalid", false, 0);
  check_parser("99999999999999999999999999999999999999", false, 0);
  char number[64];
  snprintf(number, sizeof(number), "%ld", LONG_MAX); check_parser(number, true, LONG_MAX);
  snprintf(number, sizeof(number), "%ld", LONG_MIN); check_parser(number, true, LONG_MIN);
  long value = 99;
  assert(!_mi_strntol(NULL, 10, NULL, &value) && value == 0);

  const char* inputs[] = { "2097152\n", "8388608\n", "33554432\n", "536870912\n", "", "bad", "-1", "0", NULL };
  const size_t expected[] = { 2*MI_MiB, 8*MI_MiB, 32*MI_MiB, 512*MI_MiB, 2*MI_MiB, 2*MI_MiB, 2*MI_MiB, 2*MI_MiB, 2*MI_MiB };
  for (size_t i = 0; i < sizeof(inputs)/sizeof(inputs[0]); i++) {
    pmd_text = inputs[i];
    assert(unix_detect_large_page_size() == expected[i]);
  }

  #if defined(MI_NO_THP)
  const int positive = 0;
  #else
  const int positive = MADV_HUGEPAGE;
  #endif
  #if defined(TEST_HAS_NOHUGEPAGE)
  const int negative = MADV_NOHUGEPAGE;
  #else
  const int negative = 0;
  #endif
  check_advice(2*MI_MiB, 2*MI_MiB, true, true, true, positive);
  check_advice(8*MI_MiB, 8*MI_MiB, true, true, true, positive);
  check_advice(32*MI_MiB, 32*MI_MiB, true, true, true, negative);
  check_advice(512*MI_MiB, 512*MI_MiB, true, true, true, negative);
  check_advice(512*MI_MiB, 512*MI_MiB, false, true, true, negative);
  check_advice(512*MI_MiB, 512*MI_MiB, true, false, true, 0);
  check_advice(512*MI_MiB, 512*MI_MiB, true, true, false, negative);
  check_advice(512*MI_MiB, 512*MI_MiB-1, true, true, true, 0);
  check_advice(512*MI_MiB, MI_MiB, true, true, true, 0);
  option_enabled = false;
  check_advice(2*MI_MiB, 2*MI_MiB, true, true, true, 0);
  check_advice(512*MI_MiB, 512*MI_MiB, true, true, true, negative);
  puts("bounded parser, PMD size/fallback and THP advice controls passed");
}
