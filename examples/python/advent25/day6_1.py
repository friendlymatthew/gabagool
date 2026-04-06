import os   

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day6.txt")) as f:  
    input = f.read()

lines = input.splitlines()

ops = lines[-1]

indices = []
binops = []

for i, ch in enumerate(ops):
    if ch in '+*':
        indices.append(i)
        binops.append(ch)

indices.append(len(ops))

table = []

for i in range(len(indices) - 1):
    table.append([0] * (len(lines) - 1))


for i, line in enumerate(lines[:len(lines) - 1]):
    for j in range(len(indices) - 1):
        start, end = indices[j], indices[j + 1]

        n = int(line[start:end])
        table[j][i] = n

# print(table)

out = 0

for col, op in zip(table, binops):
    if op == '+':
        out += sum(col)
    else:
        n = 1
        for c in col:
            n *= c
        out += n

print(out)