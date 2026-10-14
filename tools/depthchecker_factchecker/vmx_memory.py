"""Read a stopped Hull's guest virtual memory through CR3 and EPT.

Every physical read uses the host HHDM alias while stopped at the L1 debug
handoff. No inferior function is executed. Unsupported/unmapped translations
fail explicitly; host low virtual mappings are never substituted.
"""
import struct

ADDRESS_MASK = 0x000f_ffff_ffff_f000


class GuestMemory:
    def __init__(self, host_read, guest_cr3, eptp, hhdm_offset, guest_cr4=0):
        self.host_read = host_read
        self.cr3, self.eptp, self.hhdm = int(guest_cr3), int(eptp), int(hhdm_offset)
        if int(guest_cr4) & (1 << 12):
            raise ValueError("Five-level guest paging is not supported by this capture reader")
        if ((self.eptp >> 3) & 7) + 1 != 4:
            raise ValueError("Capture reader requires four-level EPT")
        self.entries = {}
        self.pages = {}

    def host_word(self, physical):
        address = self.hhdm + int(physical)
        if address not in self.entries:
            self.entries[address] = struct.unpack("<Q", bytes(self.host_read(address, 8)))[0]
        return self.entries[address]

    def ept_translate(self, guest_physical):
        physical = int(guest_physical)
        table = self.eptp & ADDRESS_MASK
        for shift in (39, 30, 21, 12):
            entry = self.host_word(table + ((physical >> shift) & 511) * 8)
            if not entry & 7:
                raise OSError("Unmapped EPT guest physical address 0x%x" % physical)
            if shift in (30, 21) and entry & (1 << 7):
                return (entry & ADDRESS_MASK & ~((1 << shift) - 1)) + (physical & ((1 << shift) - 1))
            table = entry & ADDRESS_MASK
        return table + (physical & 4095)

    def translate(self, virtual):
        virtual = int(virtual)
        if not 0 <= virtual <= 0xffff_ffff_ffff_ffff or (virtual >> 48) != (0xffff if virtual & (1 << 47) else 0):
            raise ValueError("Noncanonical guest virtual address 0x%x" % virtual)
        page = virtual & -4096
        if page in self.pages:
            return self.pages[page] + (virtual & 4095)
        table = self.cr3 & ADDRESS_MASK
        for shift in (39, 30, 21, 12):
            entry_address = self.ept_translate(table + ((virtual >> shift) & 511) * 8)
            entry = self.host_word(entry_address)
            if not entry & 1:
                raise OSError("Unmapped guest virtual address 0x%x" % virtual)
            if shift in (30, 21) and entry & (1 << 7):
                guest_physical = (entry & ADDRESS_MASK & ~((1 << shift) - 1)) + (virtual & ((1 << shift) - 1))
                break
            table = entry & ADDRESS_MASK
        else:
            guest_physical = table + (virtual & 4095)
        physical = self.ept_translate(guest_physical)
        self.pages[page] = physical - (virtual & 4095)
        return physical

    def read_memory(self, virtual, size):
        virtual, size = int(virtual), int(size)
        result = bytearray()
        while size:
            count = min(size, 4096 - (virtual & 4095))
            physical = self.translate(virtual)
            data = bytes(self.host_read(self.hhdm + physical, count))
            if len(data) != count:
                raise OSError("Short host HHDM memory read")
            result.extend(data)
            virtual += count
            size -= count
        return bytes(result)
