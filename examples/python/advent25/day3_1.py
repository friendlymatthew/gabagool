import os                                                                  

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day3.txt")) as f:  
    input = f.read()

lines = input.splitlines()

def largest(line: str) -> int:
    greatest = (-float('inf'), -float('inf'))

    for i, ch in enumerate(line):
        if i == len(line) - 1:
            continue

        g_i, g_n = greatest

        if g_n < int(ch):
            greatest = (i, int(ch))

    greatest_i, greatest_n = greatest

    second = -float('inf')

    for ch in line[greatest_i+1:]:
        if second < int(ch):
            second = int(ch)

   
    return greatest_n * 10 + second

out = sum(map(largest, lines))
print(out)