// Mandelbrot set — heavy f64 arithmetic (add, mul, comparisons)
#define WIDTH 128
#define HEIGHT 128
#define MAX_ITER 100

int mandelbrot_bench(void) {
    int total = 0;

    for (int py = 0; py < HEIGHT; py++) {
        for (int px = 0; px < WIDTH; px++) {
            double x0 = (double)px / WIDTH * 3.5 - 2.5;
            double y0 = (double)py / HEIGHT * 2.0 - 1.0;

            double x = 0.0;
            double y = 0.0;
            int iter = 0;

            while (x * x + y * y <= 4.0 && iter < MAX_ITER) {
                double xtemp = x * x - y * y + x0;
                y = 2.0 * x * y + y0;
                x = xtemp;
                iter++;
            }

            total += iter;
        }
    }

    return total;
}
