import sys

data = bytearray(200_000_000)
for i in range(0, len(data), 4096):
    data[i] = i & 0xff

print(f"allocated {len(data)} bytes", file=sys.stderr)
