#include <stddef.h>
#include "stubs.h"

// https://wiki.osdev.org/C%2B%2B

// new and delete (not used by driver)
void *operator new(size_t size)
{
	return gpgpu_aligned_alloc(0, size);
}

void *operator new[](size_t size)
{
	return gpgpu_aligned_alloc(0, size);
}

void operator delete(void *p)
{
	free(p);
}

void operator delete[](void *p)
{
	free(p);
}

void operator delete(void *p, unsigned long s)
{
	(void)s;
	free(p);
}

void operator delete[](void *p, unsigned long s)
{
	(void)s;
	free(p);
}

// other c++ stuff
#define ATEXIT_MAX_FUNCS 128
extern "C"
{
	void *__dso_handle = 0; // Attention! Optimally, you should remove the '= 0' part and define this in your asm script.
	typedef unsigned uarch_t;
	struct atexit_func_entry_t
	{
		/*
		 * Each member is at least 4 bytes large. Such that each entry is 12bytes.
		 * 128 * 12 = 1.5KB exact.
		 **/
		void (*destructor_func)(void *);
		void *obj_ptr;
		void *dso_handle;
	};

	atexit_func_entry_t __atexit_funcs[ATEXIT_MAX_FUNCS];
	uarch_t __atexit_func_count = 0;

	int __cxa_atexit(void (*f)(void *), void *objptr, void *dso)
	{
		if (__atexit_func_count >= ATEXIT_MAX_FUNCS)
		{
			return -1;
		};
		__atexit_funcs[__atexit_func_count].destructor_func = f;
		__atexit_funcs[__atexit_func_count].obj_ptr = objptr;
		__atexit_funcs[__atexit_func_count].dso_handle = dso;
		__atexit_func_count++;
		return 0; /*I would prefer if functions returned 1 on success, but the ABI says...*/
	}
}

namespace __cxxabiv1
{
	/* guard variables */

	/* The ABI requires a 64-bit type.  */
	__extension__ typedef int __guard __attribute__((mode(__DI__)));

	extern "C" int __cxa_guard_acquire(__guard *);
	extern "C" void __cxa_guard_release(__guard *);
	extern "C" void __cxa_guard_abort(__guard *);

	extern "C" int __cxa_guard_acquire(__guard *g)
	{
		return !*(char *)(g);
	}

	extern "C" void __cxa_guard_release(__guard *g)
	{
		*(char *)g = 1;
	}

	extern "C" void __cxa_guard_abort(__guard *)
	{
	}
}
