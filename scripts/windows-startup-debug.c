// Run the isolated startup test as a debuggee and symbolize its first AV.
// The debugger has its own initialized CRT, so reporting also works before main.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <dbghelp.h>
#include <stdio.h>

static DWORD64 image_base;

static void print_stack(HANDLE process, HANDLE thread, const char* symbol_path) {
  CONTEXT context = {0};
  context.ContextFlags = CONTEXT_FULL;
  if (!GetThreadContext(thread, &context)) {
    printf("GetThreadContext failed: %lu\n", GetLastError());
    return;
  }
  printf("RIP=%llx RSP=%llx RBP=%llx RCX=%llx RDX=%llx\n",
         (unsigned long long)context.Rip, (unsigned long long)context.Rsp,
         (unsigned long long)context.Rbp, (unsigned long long)context.Rcx,
         (unsigned long long)context.Rdx);
  printf("AV_RVA=%llx\n", (unsigned long long)(context.Rip - image_base));
  SymSetOptions(SYMOPT_LOAD_LINES | SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS |
                SYMOPT_FAIL_CRITICAL_ERRORS);
  if (!SymInitialize(process, symbol_path, TRUE)) {
    printf("SymInitialize failed: %lu\n", GetLastError());
    return;
  }
  STACKFRAME64 frame = {0};
  frame.AddrPC.Offset = context.Rip;
  frame.AddrStack.Offset = context.Rsp;
  frame.AddrFrame.Offset = context.Rbp;
  frame.AddrPC.Mode = frame.AddrStack.Mode = frame.AddrFrame.Mode = AddrModeFlat;
  for (unsigned int i = 0; i < 48 && frame.AddrPC.Offset != 0; i++) {
    union {
      SYMBOL_INFO alignment;
      char bytes[sizeof(SYMBOL_INFO) + MAX_SYM_NAME];
    } storage = {0};
    SYMBOL_INFO* symbol = (SYMBOL_INFO*)storage.bytes;
    symbol->SizeOfStruct = sizeof(SYMBOL_INFO);
    symbol->MaxNameLen = MAX_SYM_NAME;
    DWORD64 displacement = 0;
    IMAGEHLP_LINE64 line = {0};
    line.SizeOfStruct = sizeof(line);
    DWORD line_displacement = 0;
    printf("#%u %llx", i, (unsigned long long)frame.AddrPC.Offset);
    printf(" RVA=%llx", (unsigned long long)(frame.AddrPC.Offset - image_base));
    if (SymFromAddr(process, frame.AddrPC.Offset, &displacement, symbol)) {
      printf(" %s+%llu", symbol->Name, (unsigned long long)displacement);
    }
    if (SymGetLineFromAddr64(process, frame.AddrPC.Offset, &line_displacement, &line)) {
      printf(" at %s:%lu", line.FileName, line.LineNumber);
    }
    putchar('\n');
    if (!StackWalk64(IMAGE_FILE_MACHINE_AMD64, process, thread, &frame, &context,
                     NULL, SymFunctionTableAccess64, SymGetModuleBase64, NULL)) break;
  }
  SymCleanup(process);
}

int wmain(int argc, wchar_t** argv) {
  if (argc != 2) return 2;
  wchar_t symbol_directory[MAX_PATH];
  if (wcsncpy_s(symbol_directory, MAX_PATH, argv[1], _TRUNCATE) != 0) return 2;
  wchar_t* last_separator = wcsrchr(symbol_directory, L'\\');
  if (last_separator == NULL) return 2;
  *last_separator = L'\0';
  char symbol_path[MAX_PATH * 3];
  if (WideCharToMultiByte(CP_UTF8, 0, symbol_directory, -1, symbol_path,
                          sizeof(symbol_path), NULL, NULL) == 0) return 2;
  STARTUPINFOW startup = {0};
  startup.cb = sizeof(startup);
  PROCESS_INFORMATION child = {0};
  if (!CreateProcessW(argv[1], NULL, NULL, NULL, FALSE, DEBUG_ONLY_THIS_PROCESS,
                      NULL, NULL, &startup, &child)) {
    printf("CreateProcess failed: %lu\n", GetLastError());
    return 2;
  }
  int result = 0;
  DEBUG_EVENT event;
  for (;;) {
    if (!WaitForDebugEvent(&event, 120000)) {
      printf("Debug event wait failed: %lu\n", GetLastError());
      TerminateProcess(child.hProcess, 2);
      result = 2;
      break;
    }
    DWORD disposition = DBG_CONTINUE;
    if (event.dwDebugEventCode == CREATE_PROCESS_DEBUG_EVENT) {
      image_base = (DWORD64)event.u.CreateProcessInfo.lpBaseOfImage;
      if (event.u.CreateProcessInfo.hFile) CloseHandle(event.u.CreateProcessInfo.hFile);
    }
    if (event.dwDebugEventCode == LOAD_DLL_DEBUG_EVENT && event.u.LoadDll.hFile) {
      CloseHandle(event.u.LoadDll.hFile);
    }
    if (event.dwDebugEventCode == EXCEPTION_DEBUG_EVENT) {
      EXCEPTION_RECORD* exception = &event.u.Exception.ExceptionRecord;
      if (exception->ExceptionCode == EXCEPTION_ACCESS_VIOLATION) {
        if (event.u.Exception.dwFirstChance) {
          // Let the process's vectored and structured handlers inspect it.
          disposition = DBG_EXCEPTION_NOT_HANDLED;
        } else {
          printf("Unhandled AV at %p; access=%llu address=%llx\n",
                 exception->ExceptionAddress,
                 (unsigned long long)exception->ExceptionInformation[0],
                 (unsigned long long)exception->ExceptionInformation[1]);
          MEMORY_BASIC_INFORMATION memory = {0};
          VirtualQueryEx(child.hProcess, (void*)exception->ExceptionInformation[1],
                         &memory, sizeof(memory));
          printf("Fault memory: RVA=%llx state=%lx protect=%lx type=%lx region=%p size=%llu\n",
                 (unsigned long long)(exception->ExceptionInformation[1] - image_base),
                 memory.State, memory.Protect, memory.Type, memory.BaseAddress,
                 (unsigned long long)memory.RegionSize);
          unsigned char instructions[32];
          SIZE_T count = 0;
          if (ReadProcessMemory(child.hProcess, exception->ExceptionAddress,
                                instructions, sizeof(instructions), &count)) {
            printf("Fault instruction bytes:");
            for (SIZE_T i = 0; i < count; i++) printf(" %02x", instructions[i]);
            putchar('\n');
          }
          HANDLE thread = OpenThread(THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                                     FALSE, event.dwThreadId);
          if (thread) {
            print_stack(child.hProcess, thread, symbol_path);
            CloseHandle(thread);
          }
          fflush(stdout);
          TerminateProcess(child.hProcess, exception->ExceptionCode);
          result = 1;
        }
      } else if (exception->ExceptionCode != EXCEPTION_BREAKPOINT) {
        disposition = DBG_EXCEPTION_NOT_HANDLED;
      }
    }
    if (event.dwDebugEventCode == EXIT_PROCESS_DEBUG_EVENT) {
      if (event.u.ExitProcess.dwExitCode != 0) result = 1;
      ContinueDebugEvent(event.dwProcessId, event.dwThreadId, disposition);
      break;
    }
    ContinueDebugEvent(event.dwProcessId, event.dwThreadId, disposition);
  }
  CloseHandle(child.hThread);
  CloseHandle(child.hProcess);
  return result;
}
