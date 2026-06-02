#ifndef __LIBMX_CHAIN_H__
#define __LIBMX_CHAIN_H__

class Chain
{
private:
	Chain(const Chain &copy);

public:
	Chain() : next(nullptr) {}
	Chain *volatile next;
};

#endif //__LIBMX_CHAIN_H__
