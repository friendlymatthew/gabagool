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

        if j + 1 < len(indices) - 1:
            end -= 1

        n = list(line[start:end])
        table[j][i] = n


out = 0


for op, col in zip(binops, table):

    tmp = [[''] * len(col) for _ in range(len(col[0]))]


    for i in range(len(col)):
        row = col[i]

        for j in range(len(row)):
            tmp[j][i] = col[i][j]
        
    ans = 0 if op == "+" else 1

    for t in tmp:
        s = int(''.join([ch for ch in t if ch != ' ']))
        
        if op == "+":
            ans += s
        else:
            ans *= s
    
    out += ans

print(out)


