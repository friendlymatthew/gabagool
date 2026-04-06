import os                                                                  

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day2.txt")) as f:  
    input = f.read()

ranges = input.split(',')

def count_invalid(r: str) -> int:
    start, end = r.split('-')

    out = 0
    for n in range(int(start), int(end) + 1):
        s = str(n)

        if len(s) % 2 == 0 and s[:len(s) // 2] == s[len(s)// 2:]:
            out += n

    return out

out = sum(map(count_invalid, ranges))
print(out)