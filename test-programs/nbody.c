// N-body simulation — f64 math + struct-of-arrays memory layout
#define NUM_BODIES 5
#define STEPS 100000

static double px[NUM_BODIES], py[NUM_BODIES], pz[NUM_BODIES];
static double vx[NUM_BODIES], vy[NUM_BODIES], vz[NUM_BODIES];
static double mass[NUM_BODIES];

static double sqrt_approx(double x) {
    // Newton's method for sqrt
    if (x <= 0.0) return 0.0;
    double guess = x;
    for (int i = 0; i < 20; i++) {
        guess = 0.5 * (guess + x / guess);
    }
    return guess;
}

static void init_bodies(void) {
    double pi = 3.141592653589793;
    double solar_mass = 4.0 * pi * pi;
    double days_per_year = 365.24;

    // Sun
    px[0] = 0; py[0] = 0; pz[0] = 0;
    vx[0] = 0; vy[0] = 0; vz[0] = 0;
    mass[0] = solar_mass;

    // Jupiter
    px[1] = 4.84143144246472090;
    py[1] = -1.16032004402742839;
    pz[1] = -0.103622044471123109;
    vx[1] = 0.00166007664274403694 * days_per_year;
    vy[1] = 0.00769901118419740425 * days_per_year;
    vz[1] = -0.0000690460016972063023 * days_per_year;
    mass[1] = 0.000954791938424326609 * solar_mass;

    // Saturn
    px[2] = 8.34336671824457987;
    py[2] = 4.12479856412430479;
    pz[2] = -0.403523417114321381;
    vx[2] = -0.00276742510726862411 * days_per_year;
    vy[2] = 0.00499852801234917238 * days_per_year;
    vz[2] = 0.0000230417297573763929 * days_per_year;
    mass[2] = 0.000285885980666130812 * solar_mass;

    // Uranus
    px[3] = 12.8943695621391310;
    py[3] = -15.1111514016986312;
    pz[3] = -0.223307578892655734;
    vx[3] = 0.00296460137564761618 * days_per_year;
    vy[3] = 0.00237847173959480950 * days_per_year;
    vz[3] = -0.0000296589568540237556 * days_per_year;
    mass[3] = 0.0000436624404335156298 * solar_mass;

    // Neptune
    px[4] = 15.3796971148509165;
    py[4] = -25.9193146099879641;
    pz[4] = 0.179258772950371181;
    vx[4] = 0.00268067772490389322 * days_per_year;
    vy[4] = 0.00162824170038242295 * days_per_year;
    vz[4] = -0.0000951592254519715870 * days_per_year;
    mass[4] = 0.0000515138902046611451 * solar_mass;

    // Offset momentum
    double totpx = 0, totpy = 0, totpz = 0;
    for (int i = 0; i < NUM_BODIES; i++) {
        totpx += vx[i] * mass[i];
        totpy += vy[i] * mass[i];
        totpz += vz[i] * mass[i];
    }
    vx[0] = -totpx / solar_mass;
    vy[0] = -totpy / solar_mass;
    vz[0] = -totpz / solar_mass;
}

static void advance(double dt) {
    for (int i = 0; i < NUM_BODIES; i++) {
        for (int j = i + 1; j < NUM_BODIES; j++) {
            double dx = px[i] - px[j];
            double dy = py[i] - py[j];
            double dz = pz[i] - pz[j];
            double dist2 = dx * dx + dy * dy + dz * dz;
            double dist = sqrt_approx(dist2);
            double mag = dt / (dist2 * dist);

            vx[i] -= dx * mass[j] * mag;
            vy[i] -= dy * mass[j] * mag;
            vz[i] -= dz * mass[j] * mag;

            vx[j] += dx * mass[i] * mag;
            vy[j] += dy * mass[i] * mag;
            vz[j] += dz * mass[i] * mag;
        }
    }

    for (int i = 0; i < NUM_BODIES; i++) {
        px[i] += dt * vx[i];
        py[i] += dt * vy[i];
        pz[i] += dt * vz[i];
    }
}

static double energy(void) {
    double e = 0.0;
    for (int i = 0; i < NUM_BODIES; i++) {
        e += 0.5 * mass[i] * (vx[i]*vx[i] + vy[i]*vy[i] + vz[i]*vz[i]);
        for (int j = i + 1; j < NUM_BODIES; j++) {
            double dx = px[i] - px[j];
            double dy = py[i] - py[j];
            double dz = pz[i] - pz[j];
            double dist = sqrt_approx(dx*dx + dy*dy + dz*dz);
            e -= (mass[i] * mass[j]) / dist;
        }
    }
    return e;
}

// Returns energy * 1e9 truncated to int for checksum
int nbody_bench(void) {
    init_bodies();
    for (int i = 0; i < STEPS; i++) {
        advance(0.01);
    }
    double e = energy();
    return (int)(e * 1000000000.0);
}
