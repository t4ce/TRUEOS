#ifndef CPP_SUPPORT_H
#define CPP_SUPPORT_H

// placement new
inline void *operator new(unsigned long, void *p) throw() { return p; }
inline void *operator new[](unsigned long, void *p) throw() { return p; }
inline void operator delete(void *, void *) throw() {}
inline void operator delete[](void *, void *) throw() {}

#endif // CPP_SUPPORT_H