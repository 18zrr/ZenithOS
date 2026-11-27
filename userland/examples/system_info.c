#include "zenith.h"

void _start(void)
{
    char title[96];
    if (zen_abi_version() != 1 || zen_uptime_ticks() < 0 || zen_heap_bytes() < 0)
        zen_exit(1);
    long n = zen_release(title, sizeof title);
    if (n <= 0 || zen_write(title, n) != n)
        zen_exit(2);
    zen_write("\n", 1);
    if (zen_notify("System info completed", sizeof("System info completed") - 1) != 0)
        zen_exit(3);
    zen_exit(0);
}
