/* Linux/x86-64 direct KVM bring-up launcher. No firmware or QEMU dependency.
 * Supplies a deliberately limited Limine-shaped handoff, not a conformant
 * implementation of Limine. See README.md for the guest platform boundary. */
#define _GNU_SOURCE
#include <elf.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <linux/kvm.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define KERNEL_OFFSET UINT64_C(0xffffffff80000000)
#define HHDM UINT64_C(0xffff800000000000)
#define LOW_END UINT64_C(0xff000)
#define PAGE 4096
#define MIB (UINT64_C(1024) * 1024)

static uint8_t *ram;
static uint64_t ram_size, arena = 0x10000;
static volatile sig_atomic_t stopped;
static volatile struct kvm_run *active_run;
static void stop(int sig) {
    stopped = sig;
    if (active_run) active_run->immediate_exit = 1;
}
static void die(const char *s) { fprintf(stderr, "trueos-kvm: %s\n", s); exit(1); }
static void syserr(const char *s) { perror(s); exit(1); }
static int checked(int fd, unsigned long op, void *arg, const char *name) {
    int n = ioctl(fd, op, arg);
    if (n < 0) syserr(name);
    return n;
}
static bool range(uint64_t off, uint64_t len, uint64_t size) {
    return off <= size && len <= size - off;
}
static uint64_t alloc_guest(size_t len, size_t alignment) {
    arena = (arena + alignment - 1) & ~(uint64_t)(alignment - 1);
    if (!range(arena, len, LOW_END)) die("boot handoff exceeds reserved low RAM");
    uint64_t p = arena;
    arena += len;
    return p;
}
static uint64_t words(const uint64_t *data, size_t count) {
    uint64_t p = alloc_guest(count * 8, 8);
    memcpy(ram + p, data, count * 8);
    return HHDM + p;
}

/* Identity + HHDM map of 4 GiB, and the kernel's fixed upper 2 GiB mapping.
 * Missing physical backing still exits to userspace for MMIO diagnostics. */
static uint64_t page_tables(void) {
    uint64_t root = alloc_guest(PAGE, PAGE);
    uint64_t low = alloc_guest(PAGE, PAGE);
    uint64_t high = alloc_guest(PAGE, PAGE);
    uint64_t *pml4 = (void *)(ram + root), *pdpt = (void *)(ram + low);
    pml4[0] = low | 3;
    pml4[256] = low | 3;
    pml4[511] = high | 3;
    for (unsigned g = 0; g < 4; ++g) {
        uint64_t pd = alloc_guest(PAGE, PAGE);
        pdpt[g] = pd | 3;
        if (g < 2) ((uint64_t *)(ram + high))[510 + g] = pd | 3;
        for (unsigned i = 0; i < 512; ++i)
            ((uint64_t *)(ram + pd))[i] = ((uint64_t)g << 30) | ((uint64_t)i << 21) | 0x83;
    }
    return root;
}

static uint64_t load_kernel(const char *path) {
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) syserr(path);
    struct stat st;
    if (fstat(fd, &st)) syserr("fstat ELF");
    if (st.st_size < (off_t)sizeof(Elf64_Ehdr)) die("truncated ELF header");
    size_t size = (size_t)st.st_size;
    uint8_t *file = mmap(NULL, size, PROT_READ, MAP_PRIVATE, fd, 0);
    if (file == MAP_FAILED) syserr("mmap ELF");
    Elf64_Ehdr eh;
    memcpy(&eh, file, sizeof eh);
    if (memcmp(eh.e_ident, ELFMAG, SELFMAG) || eh.e_ident[EI_CLASS] != ELFCLASS64 ||
        eh.e_ident[EI_DATA] != ELFDATA2LSB || eh.e_machine != EM_X86_64 ||
        eh.e_type != ET_EXEC || eh.e_phentsize != sizeof(Elf64_Phdr) ||
        !range(eh.e_phoff, (uint64_t)eh.e_phnum * sizeof(Elf64_Phdr), size))
        die("expected a fixed-address x86-64 little-endian executable ELF");
    uint64_t first = UINT64_MAX, end = 0;
    bool entry_ok = false;
    for (unsigned i = 0; i < eh.e_phnum; ++i) {
        Elf64_Phdr p;
        memcpy(&p, file + eh.e_phoff + i * sizeof p, sizeof p);
        if (p.p_type != PT_LOAD) continue;
        if (p.p_filesz > p.p_memsz || !range(p.p_offset, p.p_filesz, size) ||
            p.p_paddr < LOW_END || !range(p.p_paddr, p.p_memsz, ram_size) ||
            p.p_vaddr != KERNEL_OFFSET + p.p_paddr)
            die("ELF segment is outside RAM or violates the TRUEOS fixed mapping");
        if (p.p_paddr < first) first = p.p_paddr;
        if (p.p_paddr + p.p_memsz > end) end = p.p_paddr + p.p_memsz;
        if ((p.p_flags & PF_X) && range(eh.e_entry - p.p_vaddr, 1, p.p_memsz)) entry_ok = true;
        memcpy(ram + p.p_paddr, file + p.p_offset, p.p_filesz);
        memset(ram + p.p_paddr + p.p_filesz, 0, p.p_memsz - p.p_filesz);
    }
    if (!entry_ok || end >= ram_size - 64 * MIB) die("invalid entry or insufficient free guest RAM");
    end = (end + PAGE - 1) & ~(uint64_t)(PAGE - 1);

    /* Restrict request discovery to the named section, never scan code for magic. */
    if (eh.e_shentsize != sizeof(Elf64_Shdr) || eh.e_shstrndx >= eh.e_shnum ||
        !range(eh.e_shoff, (uint64_t)eh.e_shnum * sizeof(Elf64_Shdr), size))
        die("ELF needs section headers for .limine_requests");
    Elf64_Shdr names;
    memcpy(&names, file + eh.e_shoff + eh.e_shstrndx * sizeof names, sizeof names);
    if (!range(names.sh_offset, names.sh_size, size)) die("invalid ELF section names");
    Elf64_Shdr requests = {0};
    for (unsigned i = 0; i < eh.e_shnum; ++i) {
        Elf64_Shdr s;
        memcpy(&s, file + eh.e_shoff + i * sizeof s, sizeof s);
        if (s.sh_name >= names.sh_size) die("invalid ELF section name offset");
        const char *name = (const char *)file + names.sh_offset + s.sh_name;
        if (!memchr(name, 0, names.sh_size - s.sh_name)) die("unterminated ELF section name");
        if (!strcmp(name, ".limine_requests")) requests = s;
    }
    if (!(requests.sh_flags & SHF_ALLOC) || (requests.sh_addr & 7) || requests.sh_addr < KERNEL_OFFSET + first ||
        !range(requests.sh_addr - KERNEL_OFFSET, requests.sh_size, end))
        die("missing or invalid .limine_requests section");

    uint64_t map_ptrs[4], count = 0;
    const uint64_t entries[][3] = {
        {0, LOW_END, 1},
        {LOW_END, first - LOW_END, 0},
        {first, end - first, 6},
        {end, ram_size - end, 0},
    };
    for (unsigned i = 0; i < 4; ++i)
        if (entries[i][1]) map_ptrs[count++] = words(entries[i], 3);
    uint64_t ptrs = words(map_ptrs, count);
    uint64_t mmap_response[] = {0, count, ptrs};
    uint64_t memmap = words(mmap_response, 3);
    uint64_t hhdm = words((uint64_t[]){0, HHDM}, 2);
    uint64_t executable = words((uint64_t[]){0, first, KERNEL_OFFSET + first}, 3);
    uint64_t date = words((uint64_t[]){0, (uint64_t)time(NULL)}, 2);
    const char cmdline[] = "timezone=Europe/Berlin keyboard=de trueos.platform=kvm-direct";
    uint64_t cmd = alloc_guest(sizeof cmdline, 8);
    memcpy(ram + cmd, cmdline, sizeof cmdline);
    uint64_t command = words((uint64_t[]){0, HHDM + cmd}, 2);
    unsigned answered = 0;
    uint64_t start = requests.sh_addr - KERNEL_OFFSET;
    for (uint64_t i = 0; i + 48 <= requests.sh_size; i += 8) {
        uint64_t *r = (void *)(ram + start + i);
        if (r[0] != UINT64_C(0xc7b1dd30df4c8b88) || r[1] != UINT64_C(0x0a82e883a194f07b)) continue;
        uint64_t response = 0;
#define REQUEST(a, b, value) if (r[2] == UINT64_C(a) && r[3] == UINT64_C(b)) response = value
        REQUEST(0x48dcf1cb8ad2b852, 0x63984e959a98244b, hhdm);
        REQUEST(0x67cf3d9d378a806f, 0xe304acdfc50c3c62, memmap);
        REQUEST(0x71ba76863cc55f63, 0xb2644a48c516a487, executable);
        REQUEST(0x502746e184c088aa, 0xfbc5ec83e6327893, date);
        REQUEST(0x4b161536e598651e, 0xb390ad4a2f1f303a, command);
#undef REQUEST
        r[5] = response;
        if (response) ++answered;
    }
    if (answered != 5) die("kernel does not contain the five supported boot requests");
    /* Leave BaseRevision unsupported: this shim does not claim full protocol compliance. */
    fprintf(stderr, "trueos-kvm: ELF %s, entry=%#" PRIx64 ", image=[%#" PRIx64 ",%#" PRIx64 "), handoff=%u responses\n",
            path, eh.e_entry, first, end, answered);
    munmap(file, size);
    close(fd);
    return eh.e_entry;
}

static struct kvm_segment segment(uint16_t selector, bool code) {
    return (struct kvm_segment){.base = 0, .limit = UINT32_MAX, .selector = selector,
        .type = code ? 11 : 3, .present = 1, .s = 1, .l = code, .db = !code, .g = 1};
}
static void cpu_setup(int kvm, int cpu, uint64_t entry, uint64_t root) {
    size_t cpuid_size = sizeof(struct kvm_cpuid2) + 256 * sizeof(struct kvm_cpuid_entry2);
    struct kvm_cpuid2 *cpuid = calloc(1, cpuid_size);
    if (!cpuid) syserr("calloc CPUID");
    cpuid->nent = 256;
    checked(kvm, KVM_GET_SUPPORTED_CPUID, cpuid, "KVM_GET_SUPPORTED_CPUID");
    for (unsigned i = 0; i < cpuid->nent; ++i) {
        struct kvm_cpuid_entry2 *e = &cpuid->entries[i];
        /* One CPU, no nested virtualization contract in this first platform. */
        if (e->function == 1) {
            e->ebx = (e->ebx & 0xffff) | (1u << 16);
            e->edx &= ~(1u << 28);
            e->ecx = (e->ecx | (1u << 31)) & ~(1u << 5);
        }
        if (e->function == 0xb || e->function == 0x1f) { e->eax = 0; e->ebx = 1; e->edx = 0; }
    }
    checked(cpu, KVM_SET_CPUID2, cpuid, "KVM_SET_CPUID2");
    free(cpuid);
    struct kvm_sregs s;
    checked(cpu, KVM_GET_SREGS, &s, "KVM_GET_SREGS");
    s.cs = segment(8, true);
    s.ds = s.es = s.fs = s.gs = s.ss = segment(16, false);
    s.cr3 = root;
    s.cr4 = (1u << 5) | (1u << 9) | (1u << 10); /* PAE, SSE */
    s.cr0 = UINT64_C(0x80010033); /* paging, WP, NE, ET, MP, PE */
    s.efer = (1u << 8) | (1u << 10) | (1u << 11); /* LME/LMA/NXE */
    checked(cpu, KVM_SET_SREGS, &s, "KVM_SET_SREGS");
    struct kvm_regs regs = {.rip = entry, .rsp = HHDM + 0xf0000, .rflags = 2};
    checked(cpu, KVM_SET_REGS, &regs, "KVM_SET_REGS");
    struct kvm_fpu fpu = {.fcw = 0x37f, .mxcsr = 0x1f80};
    checked(cpu, KVM_SET_FPU, &fpu, "KVM_SET_FPU");
}
static void dump_cpu(int cpu) {
    struct kvm_regs r;
    struct kvm_sregs s;
    if (!ioctl(cpu, KVM_GET_REGS, &r) && !ioctl(cpu, KVM_GET_SREGS, &s)) {
        fprintf(stderr, "trueos-kvm: RIP=%#" PRIx64 " RSP=%#" PRIx64 " CR2=%#" PRIx64 " CR3=%#" PRIx64 "\n",
                (uint64_t)r.rip, (uint64_t)r.rsp, (uint64_t)s.cr2, (uint64_t)s.cr3);
        fprintf(stderr, "trueos-kvm: RCX=%#" PRIx64 " RSI=%#" PRIx64 " RDI=%#" PRIx64 " RBP=%#" PRIx64 "\n",
                (uint64_t)r.rcx, (uint64_t)r.rsi, (uint64_t)r.rdi, (uint64_t)r.rbp);
    }
}
static unsigned parse(const char *s, unsigned low, unsigned high) {
    char *end;
    errno = 0;
    unsigned long n = strtoul(s, &end, 10);
    if (errno || !*s || *end || n < low || n > high) die("numeric option out of range");
    return (unsigned)n;
}
int main(int argc, char **argv) {
    const char *kernel = NULL;
    unsigned memory = 1024, seconds = 10;
    bool self_test = false;
    for (int i = 1; i < argc; ++i) {
        if (!strcmp(argv[i], "--self-test")) self_test = true;
        else if (!strcmp(argv[i], "--memory") && i + 1 < argc) memory = parse(argv[++i], 256, 2048);
        else if (!strcmp(argv[i], "--timeout") && i + 1 < argc) seconds = parse(argv[++i], 1, 3600);
        else if (!strcmp(argv[i], "--help")) {
            puts("Usage: tools/kvm/run.sh [--memory MiB] [--timeout seconds] KERNEL.elf\n"
                 "       tools/kvm/run.sh --self-test\n"
                 "Experimental one-CPU headless platform; no firmware, PCI devices or nested VMX.");
            return 0;
        } else if (argv[i][0] == '-' || kernel) die("invalid arguments; use --help");
        else kernel = argv[i];
    }
    if ((!kernel && !self_test) || (kernel && self_test)) die("supply an ELF or --self-test; use --help");
    ram_size = (uint64_t)memory * MIB;
    ram = mmap(NULL, ram_size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (ram == MAP_FAILED) syserr("mmap guest RAM");
    uint64_t root = page_tables(), entry;
    if (self_test) {
        /* Execute from the same higher-half mapping as TRUEOS, write a marker,
         * then terminate through a test-only port. */
        const uint8_t probe[] = {0x66, 0xba, 0xe9, 0x00, 0xb0, 'K', 0xee,
                                0x66, 0xba, 0xf4, 0x00, 0xb0, 0x2a, 0xee, 0xf4};
        memcpy(ram + 0x100000, probe, sizeof probe);
        entry = KERNEL_OFFSET + 0x100000;
    } else entry = load_kernel(kernel);
    int kvm = open("/dev/kvm", O_RDWR | O_CLOEXEC);
    if (kvm < 0) syserr("open /dev/kvm (requires access to the kvm group/device ACL)");
    if (checked(kvm, KVM_GET_API_VERSION, NULL, "KVM_GET_API_VERSION") != KVM_API_VERSION) die("unsupported KVM API");
    if (checked(kvm, KVM_CHECK_EXTENSION, (void *)(uintptr_t)KVM_CAP_IMMEDIATE_EXIT, "KVM_CHECK_EXTENSION") <= 0)
        die("KVM immediate-exit support is required for reliable cancellation");
    int vm = checked(kvm, KVM_CREATE_VM, NULL, "KVM_CREATE_VM");
    struct kvm_userspace_memory_region region = {.slot = 0, .memory_size = ram_size, .userspace_addr = (uintptr_t)ram};
    checked(vm, KVM_SET_USER_MEMORY_REGION, &region, "KVM_SET_USER_MEMORY_REGION");
    checked(vm, KVM_SET_TSS_ADDR, (void *)(uintptr_t)0xfffbd000, "KVM_SET_TSS_ADDR");
    uint64_t identity = 0xfffbc000;
    checked(vm, KVM_SET_IDENTITY_MAP_ADDR, &identity, "KVM_SET_IDENTITY_MAP_ADDR");
    checked(vm, KVM_CREATE_IRQCHIP, NULL, "KVM_CREATE_IRQCHIP");
    int cpu = checked(vm, KVM_CREATE_VCPU, NULL, "KVM_CREATE_VCPU");
    cpu_setup(kvm, cpu, entry, root);
    int run_size = checked(kvm, KVM_GET_VCPU_MMAP_SIZE, NULL, "KVM_GET_VCPU_MMAP_SIZE");
    if ((size_t)run_size < sizeof(struct kvm_run)) die("invalid KVM run mapping size");
    struct kvm_run *run = mmap(NULL, (size_t)run_size, PROT_READ | PROT_WRITE, MAP_SHARED, cpu, 0);
    if (run == MAP_FAILED) syserr("mmap KVM run");
    active_run = run;
    struct sigaction action = {.sa_handler = stop};
    sigemptyset(&action.sa_mask);
    sigaction(SIGALRM, &action, NULL);
    sigaction(SIGINT, &action, NULL);
    sigaction(SIGTERM, &action, NULL);
    alarm(seconds);
    fprintf(stderr, "trueos-kvm: running one vCPU, %u MiB, timeout=%us\n", memory, seconds);
    int result = 1;
    uint8_t uart_lcr = 0;
    for (;;) {
        if (stopped) break;
        if (ioctl(cpu, KVM_RUN, 0) < 0) {
            if (errno == EINTR) continue;
            perror("KVM_RUN"); break;
        }
        if (run->exit_reason == KVM_EXIT_IO) {
            size_t length = (size_t)run->io.size * run->io.count;
            if (!range(run->io.data_offset, length, (uint64_t)run_size)) die("invalid KVM IO buffer");
            uint8_t *data = (uint8_t *)run + run->io.data_offset;
            uint16_t port = run->io.port;
            bool out = run->io.direction == KVM_EXIT_IO_OUT;
            if (self_test && out && port == 0xf4 && length == 1 && *data == 0x2a) {
                fprintf(stderr, "\ntrueos-kvm: self-test PASS (higher-half long-mode execution and IO exits)\n");
                result = 0; break;
            }
            if (out && (port == 0xe9 || (port == 0x3f8 && !(uart_lcr & 0x80)))) {
                fwrite(data, 1, length, stdout); fflush(stdout);
            } else if (port >= 0x3f8 && port <= 0x3ff && run->io.size == 1) {
                if (out && port == 0x3fb) uart_lcr = data[length - 1];
                if (!out) memset(data, port == 0x3fd ? 0x60 : port == 0x3fa ? 1 : 0, length);
            } else if (port == 0xcf8 || (port >= 0xcfc && port <= 0xcff)) {
                /* Empty PCI bus: config reads return the absent-device value. */
                if (!out) memset(data, 0xff, length);
            } else if (port == 0x80) {
                if (!out) memset(data, 0xff, length);
            } else {
                fprintf(stderr, "trueos-kvm: unsupported IO %s port=%#x size=%u count=%u\n",
                        out ? "write" : "read", port, run->io.size, run->io.count);
                break;
            }
        } else if (run->exit_reason == KVM_EXIT_MMIO) {
            fprintf(stderr, "trueos-kvm: unsupported MMIO %s address=%#" PRIx64 " size=%u\n",
                    run->mmio.is_write ? "write" : "read", (uint64_t)run->mmio.phys_addr, run->mmio.len);
            break;
        } else {
            fprintf(stderr, "trueos-kvm: guest exit=%u%s\n", run->exit_reason,
                    run->exit_reason == KVM_EXIT_SHUTDOWN ? " (triple fault/shutdown)" :
                    run->exit_reason == KVM_EXIT_HLT ? " (halt)" : "");
            if (run->exit_reason == KVM_EXIT_FAIL_ENTRY)
                fprintf(stderr, "trueos-kvm: hardware entry failure=%#" PRIx64 "\n", (uint64_t)run->fail_entry.hardware_entry_failure_reason);
            break;
        }
    }
    alarm(0);
    if (stopped) {
        fprintf(stderr, "trueos-kvm: %s\n", stopped == SIGALRM ? "time limit reached" : "interrupted");
        result = stopped == SIGALRM ? 124 : 128 + stopped;
    }
    if (result) dump_cpu(cpu);
    active_run = NULL;
    munmap(run, (size_t)run_size);
    close(cpu); close(vm); close(kvm);
    munmap(ram, ram_size);
    return result;
}
