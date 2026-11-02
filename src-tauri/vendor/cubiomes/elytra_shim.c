#include "elytra_shim.h"

#include "biomenoise.h"
#include "finders.h"
#include "generator.h"
#include "rng.h"
#include "util.h"
#include "xrms.h"

#include <stdlib.h>
#include <string.h>

#define ELYTRA_SEEDMAP_TILE_BLOCKS 512

void *elytra_gen_new(const char *mc_name, uint64_t seed)
{
    int mc = str2mc(mc_name);
    if (mc <= MC_UNDEF)
        return NULL;
    Generator *g = (Generator *) malloc(sizeof(Generator));
    if (!g)
        return NULL;
    setupGenerator(g, mc, 0);
    applySeed(g, DIM_OVERWORLD, seed);
    return g;
}

void elytra_gen_free(void *gen)
{
    free(gen);
}

int elytra_mc_from_name(const char *name)
{
    int mc = str2mc(name);
    return mc <= MC_UNDEF ? -1 : mc;
}

const char *elytra_mc_name(int mc)
{
    return mc2str(mc);
}

int elytra_biome_at(void *gen, int scale, int x, int y, int z)
{
    if (!gen)
        return -1;
    return getBiomeAt((const Generator *) gen, scale, x, y, z);
}

int elytra_gen_biomes(void *gen, int scale, int x, int z, int sx, int sz, int y,
                      int *out)
{
    if (!gen || !out || sx <= 0 || sz <= 0)
        return 1;
    // <=1.17 的层叠生成器需要额外工作区，不能只按 sx*sz 传缓冲。
    size_t len = getMinCacheSize((const Generator *) gen, scale, sx, 1, sz);
    if (len == 0)
        return 2;
    int *cache = (int *) malloc(len * sizeof(int));
    if (!cache)
        return 3;
    Range r = {scale, x, z, sx, sz, y, 1};
    int rc = genBiomes((const Generator *) gen, cache, r);
    if (rc == 0)
        memcpy(out, cache, (size_t) sx * (size_t) sz * sizeof(int));
    free(cache);
    return rc;
}

const char *elytra_biome_name(int mc, int biome)
{
    return biome2str(mc, biome);
}

int elytra_max_y_quart(int mc)
{
    // 1.18+ 最大方块 Y=319（quart 79）；旧版 255（quart 63）。
    return mc >= MC_1_18 ? 79 : 63;
}

// Structure set: overworld grid structures supported by getStructurePos().
// Strongholds/mineshafts use separate algorithms and are intentionally absent.
static const int ELYTRA_STRUCTURES[] = {
    Village,        Desert_Pyramid, Jungle_Temple, Swamp_Hut,   Igloo,
    Ocean_Ruin,     Shipwreck,      Monument,      Mansion,     Outpost,
    Ruined_Portal,  Ancient_City,   Treasure,      Trail_Ruins, Trial_Chambers,
    Abandoned_Camp,
};

int elytra_collect_structures(void *gen, int mc, uint64_t seed, int rx, int rz,
                              ElytraStructure *out, int cap)
{
    if (!gen || !out || cap <= 0)
        return 0;
    Generator *g = (Generator *) gen;
    int tile0x = rx * ELYTRA_SEEDMAP_TILE_BLOCKS;
    int tile1x = tile0x + ELYTRA_SEEDMAP_TILE_BLOCKS;
    int tile0z = rz * ELYTRA_SEEDMAP_TILE_BLOCKS;
    int tile1z = tile0z + ELYTRA_SEEDMAP_TILE_BLOCKS;
    int c0x = floordiv(tile0x, 16);
    int c1x = floordiv(tile1x - 1, 16);
    int c0z = floordiv(tile0z, 16);
    int c1z = floordiv(tile1z - 1, 16);
    int n = 0;
    size_t count = sizeof(ELYTRA_STRUCTURES) / sizeof(ELYTRA_STRUCTURES[0]);
    for (size_t i = 0; i < count; i++)
    {
        int type = ELYTRA_STRUCTURES[i];
        StructureConfig sc;
        if (!getStructureConfig(type, mc, &sc) || sc.regionSize <= 0)
            continue;
        int r0x = floordiv(c0x, sc.regionSize);
        int r1x = floordiv(c1x, sc.regionSize);
        int r0z = floordiv(c0z, sc.regionSize);
        int r1z = floordiv(c1z, sc.regionSize);
        for (int gx = r0x; gx <= r1x; gx++)
        {
            for (int gz = r0z; gz <= r1z; gz++)
            {
                Pos pos;
                if (!getStructurePos(type, mc, seed, gx, gz, &pos))
                    continue;
                if (pos.x < tile0x || pos.x >= tile1x || pos.z < tile0z ||
                    pos.z >= tile1z)
                    continue;
                if (!isViableStructurePos(type, g, pos.x, pos.z, 0))
                    continue;
                if (mc >= MC_1_18 &&
                    !isViableStructureTerrain(type, g, pos.x, pos.z))
                    continue;
                if (n >= cap)
                    return n;
                out[n].type = type;
                out[n].x = pos.x;
                out[n].z = pos.z;
                n++;
            }
        }
    }
    return n;
}

const char *elytra_structure_name(int type)
{
    return struct2str(type);
}

typedef struct
{
    int x;
    int z;
    int64_t dist;
} ElytraCandidate;

static int candidate_cmp(const void *a, const void *b)
{
    const ElytraCandidate *ca = (const ElytraCandidate *) a;
    const ElytraCandidate *cb = (const ElytraCandidate *) b;
    return (ca->dist > cb->dist) - (ca->dist < cb->dist);
}

// 保留距中心最近的 cap 个候选（cap <= 64）。
static void candidate_push(ElytraCandidate *best, int *count, int cap, int x,
                           int z, int64_t dist)
{
    if (*count < cap)
    {
        best[*count].x = x;
        best[*count].z = z;
        best[*count].dist = dist;
        (*count)++;
        return;
    }
    int maxi = 0;
    for (int i = 1; i < cap; i++)
        if (best[i].dist > best[maxi].dist)
            maxi = i;
    if (dist < best[maxi].dist)
    {
        best[maxi].x = x;
        best[maxi].z = z;
        best[maxi].dist = dist;
    }
}

static int int_abs(int v)
{
    return v < 0 ? -v : v;
}

// 当前 K 个候选中最远的距离平方；未满 K 时返回 0（不剪枝）。
static int64_t worst_candidate_dist(const ElytraCandidate *best, int count)
{
    int64_t worst = 0;
    for (int i = 0; i < count; i++)
        if (best[i].dist > worst)
            worst = best[i].dist;
    return worst;
}

// 下一环的最小可能距离平方超过当前第 K 名即停止；lower_bound 为保守下界。
static int ring_can_prune(const ElytraCandidate *best, int count, int cap,
                          int64_t lower_bound)
{
    if (count < cap || lower_bound <= 0)
        return 0;
    return lower_bound * lower_bound > worst_candidate_dist(best, count);
}

static void push_structure_candidate(int type, int mc, uint64_t seed,
                                     Generator *g, int center_x, int center_z,
                                     int64_t radius_sq, ElytraCandidate *best,
                                     int *count, int cap, int gx, int gz)
{
    Pos pos;
    if (!getStructurePos(type, mc, seed, gx, gz, &pos))
        return;
    int64_t dx = (int64_t) pos.x - center_x;
    int64_t dz = (int64_t) pos.z - center_z;
    int64_t dist = dx * dx + dz * dz;
    if (dist > radius_sq)
        return;
    if (!isViableStructurePos(type, g, pos.x, pos.z, 0))
        return;
    if (mc >= MC_1_18 &&
        !isViableStructureTerrain(type, g, pos.x, pos.z))
        return;
    candidate_push(best, count, cap, pos.x, pos.z, dist);
}

static void push_slime_candidate(uint64_t seed, int cx, int cz, int center_x,
                                 int center_z, int64_t radius_sq,
                                 ElytraCandidate *best, int *count, int cap)
{
    if (!isSlimeChunk(seed, cx, cz))
        return;
    int64_t bx = (int64_t) cx * 16 + 8 - center_x;
    int64_t bz = (int64_t) cz * 16 + 8 - center_z;
    int64_t dist = bx * bx + bz * bz;
    if (dist > radius_sq)
        return;
    candidate_push(best, count, cap, cx, cz, dist);
}

int elytra_find_structures(void *gen, int mc, uint64_t seed, int type,
                           int center_x, int center_z, int radius_blocks,
                           ElytraStructure *out, int cap)
{
    if (!gen || !out || cap <= 0)
        return 0;
    StructureConfig sc;
    // regionSize < 4 的结构（如埋藏的宝藏）区域过密，快速查找会退化为全图扫描。
    if (!getStructureConfig(type, mc, &sc) || sc.regionSize < 4)
        return 0;
    if (radius_blocks < 1000)
        radius_blocks = 1000;
    if (radius_blocks > 50000)
        radius_blocks = 50000;
    int c0x = floordiv(center_x - radius_blocks, 16);
    int c1x = floordiv(center_x + radius_blocks, 16);
    int c0z = floordiv(center_z - radius_blocks, 16);
    int c1z = floordiv(center_z + radius_blocks, 16);
    int r0x = floordiv(c0x, sc.regionSize);
    int r1x = floordiv(c1x, sc.regionSize);
    int r0z = floordiv(c0z, sc.regionSize);
    int r1z = floordiv(c1z, sc.regionSize);
    int cell_blocks = sc.regionSize * 16;
    int ccx = floordiv(center_x, cell_blocks);
    int ccz = floordiv(center_z, cell_blocks);
    int max_ring = int_abs(ccx - r0x);
    int other = int_abs(ccx - r1x);
    if (other > max_ring)
        max_ring = other;
    other = int_abs(ccz - r0z);
    if (other > max_ring)
        max_ring = other;
    other = int_abs(ccz - r1z);
    if (other > max_ring)
        max_ring = other;
    int best_cap = cap > 64 ? 64 : cap;
    ElytraCandidate best[64];
    int count = 0;
    int64_t radius_sq = (int64_t) radius_blocks * radius_blocks;
    Generator *g = (Generator *) gen;
    for (int ring = 0; ring <= max_ring; ring++)
    {
        for (int gx = ccx - ring; gx <= ccx + ring; gx++)
        {
            if (gx < r0x || gx > r1x)
                continue;
            if (int_abs(gx - ccx) == ring)
            {
                // 上/下两条整边（包含四角）。
                for (int gz = ccz - ring; gz <= ccz + ring; gz++)
                {
                    if (gz < r0z || gz > r1z)
                        continue;
                    push_structure_candidate(type, mc, seed, g, center_x,
                                             center_z, radius_sq, best, &count,
                                             best_cap, gx, gz);
                }
            }
            else
            {
                // 中间列只取环带上下两点，避免再次遍历环内区域。
                if (ccz - ring >= r0z && ccz - ring <= r1z)
                    push_structure_candidate(type, mc, seed, g, center_x,
                                             center_z, radius_sq, best, &count,
                                             best_cap, gx, ccz - ring);
                if (ccz + ring >= r0z && ccz + ring <= r1z)
                    push_structure_candidate(type, mc, seed, g, center_x,
                                             center_z, radius_sq, best, &count,
                                             best_cap, gx, ccz + ring);
            }
        }
        // 下一环区域离中心至少 ring*cell_blocks（中心在中心 region 内）；超过第 K 名即停。
        if (ring_can_prune(best, count, best_cap,
                           (int64_t) ring * cell_blocks))
            break;
    }
    qsort(best, count, sizeof(ElytraCandidate), candidate_cmp);
    for (int i = 0; i < count; i++)
    {
        out[i].type = type;
        out[i].x = best[i].x;
        out[i].z = best[i].z;
    }
    return count;
}

int elytra_collect_slime_chunks(uint64_t seed, int rx, int rz, ElytraChunk *out,
                                int cap)
{
    if (!out || cap <= 0)
        return 0;
    int c0x = rx * (ELYTRA_SEEDMAP_TILE_BLOCKS / 16);
    int c0z = rz * (ELYTRA_SEEDMAP_TILE_BLOCKS / 16);
    int n = 0;
    for (int dx = 0; dx < ELYTRA_SEEDMAP_TILE_BLOCKS / 16; dx++)
    {
        for (int dz = 0; dz < ELYTRA_SEEDMAP_TILE_BLOCKS / 16; dz++)
        {
            int cx = c0x + dx;
            int cz = c0z + dz;
            if (!isSlimeChunk(seed, cx, cz))
                continue;
            if (n >= cap)
                return n;
            out[n].x = cx;
            out[n].z = cz;
            n++;
        }
    }
    return n;
}

int elytra_find_slime_chunks(uint64_t seed, int center_x, int center_z,
                             int radius_blocks, ElytraChunk *out, int cap)
{
    if (!out || cap <= 0)
        return 0;
    if (radius_blocks < 1000)
        radius_blocks = 1000;
    if (radius_blocks > 20000)
        radius_blocks = 20000;
    int cx = floordiv(center_x, 16);
    int cz = floordiv(center_z, 16);
    int r = radius_blocks / 16;
    if (r < 1)
        r = 1;
    int best_cap = cap > 64 ? 64 : cap;
    ElytraCandidate best[64];
    int count = 0;
    int64_t radius_sq = (int64_t) radius_blocks * radius_blocks;
    for (int ring = 0; ring <= r; ring++)
    {
        for (int dx = -ring; dx <= ring; dx++)
        {
            if (int_abs(dx) == ring)
            {
                // 上/下两条整边（包含四角）。
                for (int dz = -ring; dz <= ring; dz++)
                    push_slime_candidate(seed, cx + dx, cz + dz, center_x,
                                         center_z, radius_sq, best, &count,
                                         best_cap);
            }
            else
            {
                // 中间列只取环带上下两点，避免再次遍历环内区块。
                push_slime_candidate(seed, cx + dx, cz - ring, center_x,
                                     center_z, radius_sq, best, &count,
                                     best_cap);
                push_slime_candidate(seed, cx + dx, cz + ring, center_x,
                                     center_z, radius_sq, best, &count,
                                     best_cap);
            }
        }
        // 下一环区块到中心至少 16*(ring+1)-15 格；超过第 K 名即停。
        if (ring_can_prune(best, count, best_cap,
                           (int64_t) 16 * (ring + 1) - 15))
            break;
    }
    qsort(best, count, sizeof(ElytraCandidate), candidate_cmp);
    for (int i = 0; i < count; i++)
    {
        out[i].x = best[i].x;
        out[i].z = best[i].z;
    }
    return count;
}

int elytra_spawn(void *gen, int *x, int *z)
{
    if (!gen || !x || !z)
        return 0;
    Pos p = getSpawn((const Generator *) gen);
    *x = p.x;
    *z = p.z;
    return 1;
}
