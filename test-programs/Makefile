CLANG = /opt/homebrew/opt/llvm/bin/clang
WASM_LD = $(shell rustc --print sysroot)/lib/rustlib/aarch64-apple-darwin/bin/gcc-ld/wasm-ld

SOURCES = fibonacci.c matrix.c sieve.c sort.c \
	ackermann.c mandelbrot.c nbody.c sha256.c \
	switch_dispatch.c indirect_call.c call_chain.c \
	binary_search.c linked_list.c bulk_memory.c \
	matrix_chain.c
WASMS = $(SOURCES:.c=.wasm)

all: $(WASMS)

%.wasm: %.c
	$(CLANG) --target=wasm32 -O2 -nostdlib -c -o $*.o $<
	$(WASM_LD) --no-entry --export-all -o $@ $*.o
	rm -f $*.o

clean:
	rm -f $(WASMS) *.o

.PHONY: all clean
