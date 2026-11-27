#ifndef ZENITH_APP_H
#define ZENITH_APP_H

/* Freestanding x86_64 Zenith application ABI, version 1. */
static inline long zen_call(long number, unsigned long arg1, unsigned long arg2)
{
    long result;
    __asm__ volatile ("syscall" : "=a"(result)
                      : "0"(number), "D"(arg1), "S"(arg2)
                      : "rcx", "r11", "cc", "memory");
    return result;
}

static inline long zen_abi_version(void) { return zen_call(0, 0, 0); }
static inline void zen_exit(unsigned char code)
{
    zen_call(1, code, 0);
    __builtin_unreachable();
}
static inline long zen_write(const void *p, unsigned long n)
{
    return zen_call(2, (unsigned long)p, n);
}
static inline long zen_release(char *p, unsigned long capacity)
{
    return zen_call(3, (unsigned long)p, capacity);
}
static inline long zen_uptime_ticks(void) { return zen_call(4, 0, 0); }
static inline long zen_heap_bytes(void) { return zen_call(5, 0, 0); }
static inline long zen_notify(const char *text, unsigned long length)
{
    return zen_call(6, (unsigned long)text, length);
}

#endif
