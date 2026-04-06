import os                                                                  

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day2.txt")) as f:  
    input = f.read()

ranges = input.split(',')

def count_invalid2(r: str) -> int:
    start, end = r.split('-')

    out = 0
    for n in range(int(start), int(end) + 1):
        s = str(n)

        h = len(s) // 2

        while h > 0:
            shifted = s[h:] + s[:h]

            if shifted == s:
                out += int(s)
                break

            h -= 1

    return out

out = sum(map(count_invalid2, ranges))
print(out)
