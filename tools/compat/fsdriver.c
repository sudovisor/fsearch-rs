// Headless driver for the ORIGINAL fsearch core (libfsearch_core.a).
// Used as ground truth for `rust-anything app record` and as the C side of the benchmark.
//
//   fsdriver scan  <db_out> [--exclude-hidden] [--exclude type:scope:target:pattern]... <root>...
//   fsdriver query <db> <sort> <asc|desc> <flags> <filter|-> [--resort <sort>] <query>...
//   fsdriver dump  <db>
//   fsdriver bench <db> <runs> <sort> <query>...       (timings on stdout, ms)
//
// sort: name|path|size|mtime|ext|type   flags: bitmask of FsearchQueryFlags
#include <glib.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "fsearch_database_entry.h"
#include "fsearch_database_exclude.h"
#include "fsearch_database_exclude_manager.h"
#include "fsearch_database_file.h"
#include "fsearch_database_include.h"
#include "fsearch_database_include_manager.h"
#include "fsearch_database_index_store.h"
#include "fsearch_database_search_info.h"
#include "fsearch_database_search_view.h"
#include "fsearch_filter_manager.h"
#include "fsearch_query.h"

static FsearchDatabaseIndexProperty
parse_sort(const char *s) {
    if (!strcmp(s, "name")) return DATABASE_INDEX_PROPERTY_NAME;
    if (!strcmp(s, "path")) return DATABASE_INDEX_PROPERTY_PATH;
    if (!strcmp(s, "size")) return DATABASE_INDEX_PROPERTY_SIZE;
    if (!strcmp(s, "mtime")) return DATABASE_INDEX_PROPERTY_MODIFICATION_TIME;
    if (!strcmp(s, "ext")) return DATABASE_INDEX_PROPERTY_EXTENSION;
    if (!strcmp(s, "type")) return DATABASE_INDEX_PROPERTY_FILETYPE;
    fprintf(stderr, "bad sort: %s\n", s);
    exit(2);
}

static void
print_entry(FsearchDatabaseEntry *e) {
    g_autoptr(GString) p = db_entry_get_path_full(e);
    printf("%c %" G_GINT64_FORMAT " %" G_GINT64_FORMAT " %s\n",
           db_entry_is_folder(e) ? 'D' : 'F',
           (gint64)db_entry_get_size(e),
           (gint64)db_entry_get_mtime(e),
           p->str);
}

static FsearchDatabaseIndexStore *
load_direct(const char *db) {
    FsearchDatabaseIndexStore *store = NULL;
    if (!fsearch_database_file_load(db, NULL, &store, NULL, NULL, NULL, NULL)) {
        printf("load failed\n");
        exit(1);
    }
    return store;
}

// The original takes an exclusive flock on the database even for reading; cases run in parallel,
// so each one loads its own private copy.
static FsearchDatabaseIndexStore *
load(const char *db) {
    g_autofree char *data = NULL;
    gsize len = 0;
    if (!g_file_get_contents(db, &data, &len, NULL)) {
        printf("load failed\n");
        exit(1);
    }
    g_autofree char *tmp = g_strdup_printf("%s/fsdriver-%d.db", g_get_tmp_dir(), (int)getpid());
    g_file_set_contents(tmp, data, (gssize)len, NULL);
    FsearchDatabaseIndexStore *store = NULL;
    const bool ok = fsearch_database_file_load(tmp, NULL, &store, NULL, NULL, NULL, NULL);
    unlink(tmp);
    if (!ok) {
        printf("load failed\n");
        exit(1);
    }
    return store;
}

static void
print_view(FsearchDatabaseIndexStore *store) {
    FsearchDatabaseSearchInfo *info = fsearch_database_index_store_get_search_info(store, 1);
    const uint32_t n = info ? fsearch_database_search_info_get_num_entries(info) : 0;
    printf("= %u results (%u folders, %u files)\n",
           n,
           info ? fsearch_database_search_info_get_num_folders(info) : 0,
           info ? fsearch_database_search_info_get_num_files(info) : 0);
    FsearchDatabaseSearchView *view = fsearch_database_index_store_get_search_view(store, 1);
    for (uint32_t i = 0; view && i < n; i++) {
        print_entry(fsearch_database_search_view_get_entry_for_idx(view, i));
    }
    g_clear_pointer(&info, fsearch_database_search_info_unref);
}

static int
cmd_scan(int argc, char **argv) {
    const char *db = argv[0];
    g_autoptr(FsearchDatabaseIncludeManager) inc = fsearch_database_include_manager_new();
    g_autoptr(FsearchDatabaseExcludeManager) exc = fsearch_database_exclude_manager_new();
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--exclude-hidden")) {
            fsearch_database_exclude_manager_set_exclude_hidden(exc, TRUE);
        }
        else if (!strcmp(argv[i], "--exclude") && i + 1 < argc) {
            g_auto(GStrv) parts = g_strsplit(argv[++i], ":", 4);
            g_autoptr(FsearchDatabaseExclude) e = fsearch_database_exclude_new(
                parts[3], TRUE,
                fsearch_database_exclude_get_type_from_string(parts[0]),
                fsearch_database_exclude_get_match_scope_from_string(parts[1]),
                fsearch_database_exclude_get_target_from_string(parts[2]));
            fsearch_database_exclude_manager_add(exc, e);
        }
        else {
            g_autoptr(FsearchDatabaseInclude) in = fsearch_database_include_new(argv[i], TRUE, FALSE, FALSE, FALSE, 0);
            fsearch_database_include_manager_add(inc, in);
        }
    }
    gint64 t0 = g_get_monotonic_time();
    g_autoptr(FsearchDatabaseIndexStore) store =
        fsearch_database_index_store_new(inc, exc, DATABASE_INDEX_PROPERTY_FLAG_DEFAULT, NULL, NULL);
    fsearch_database_index_store_start(store, NULL);
    gint64 t1 = g_get_monotonic_time();
    if (!fsearch_database_file_save(store, db)) {
        fprintf(stderr, "save failed\n");
        return 1;
    }
    gint64 t2 = g_get_monotonic_time();
    printf("folders %u files %u\n",
           fsearch_database_index_store_get_num_folders(store),
           fsearch_database_index_store_get_num_files(store));
    if (g_getenv("FSDRIVER_TIME")) {
        fprintf(stderr, "scan_ms %.3f save_ms %.3f\n", (t1 - t0) / 1000.0, (t2 - t1) / 1000.0);
    }
    return 0;
}

static int
cmd_query(int argc, char **argv) {
    const char *db = argv[0];
    FsearchDatabaseIndexProperty sort = parse_sort(argv[1]);
    FsearchSortDirection dir = !strcmp(argv[2], "desc") ? FSEARCH_SORT_DIRECTION_DESCENDING
                                                       : FSEARCH_SORT_DIRECTION_ASCENDING;
    FsearchQueryFlags flags = (FsearchQueryFlags)atoi(argv[3]);
    const char *filter_name = argv[4];
    int i = 5;
    int resort = -1;
    if (i + 1 < argc && !strcmp(argv[i], "--resort")) {
        resort = parse_sort(argv[i + 1]);
        i += 2;
    }
    g_autoptr(FsearchDatabaseIndexStore) store = load(db);
    FsearchFilterManager *filters = fsearch_filter_manager_new_with_defaults();
    FsearchFilter *filter = NULL;
    if (strcmp(filter_name, "-") != 0) {
        filter = fsearch_filter_manager_get_filter_for_name(filters, filter_name);
    }
    for (; i < argc; i++) {
        g_autoptr(FsearchQuery) q = fsearch_query_new(argv[i], filter, filters, flags, "driver");
        printf("# query \"%s\"\n", argv[i]);
        fsearch_database_index_store_search(store, 1, q, sort, dir, NULL);
        if (resort >= 0) {
            fsearch_database_index_store_sort_results(store, 1, resort, dir, NULL);
        }
        print_view(store);
    }
    if (filter) {
        fsearch_filter_unref(filter);
    }
    return 0;
}

static int
cmd_dump(int argc, char **argv) {
    g_autoptr(FsearchDatabaseIndexStore) store = load(argv[0]);
    g_autoptr(FsearchQuery) q = fsearch_query_new("", NULL, NULL, 0, "dump");
    fsearch_database_index_store_search(store, 1, q, DATABASE_INDEX_PROPERTY_PATH, FSEARCH_SORT_DIRECTION_ASCENDING, NULL);
    print_view(store);
    return 0;
}

static FsearchDatabaseIndexStore *
scan_store(int argc, char **argv) {
    g_autoptr(FsearchDatabaseIncludeManager) inc = fsearch_database_include_manager_new();
    g_autoptr(FsearchDatabaseExcludeManager) exc = fsearch_database_exclude_manager_new();
    for (int i = 0; i < argc; i++) {
        if (!strcmp(argv[i], "--exclude-hidden")) {
            fsearch_database_exclude_manager_set_exclude_hidden(exc, TRUE);
        }
        else if (!strcmp(argv[i], "--exclude") && i + 1 < argc) {
            g_auto(GStrv) parts = g_strsplit(argv[++i], ":", 4);
            g_autoptr(FsearchDatabaseExclude) e = fsearch_database_exclude_new(
                parts[3], TRUE,
                fsearch_database_exclude_get_type_from_string(parts[0]),
                fsearch_database_exclude_get_match_scope_from_string(parts[1]),
                fsearch_database_exclude_get_target_from_string(parts[2]));
            fsearch_database_exclude_manager_add(exc, e);
        }
        else {
            g_autoptr(FsearchDatabaseInclude) in = fsearch_database_include_new(argv[i], TRUE, FALSE, FALSE, FALSE, 0);
            fsearch_database_include_manager_add(inc, in);
        }
    }
    FsearchDatabaseIndexStore *store =
        fsearch_database_index_store_new(inc, exc, DATABASE_INDEX_PROPERTY_FLAG_DEFAULT, NULL, NULL);
    fsearch_database_index_store_start(store, NULL);
    // zero the run-dependent scan metadata so saved files are deterministic
    g_autoptr(FsearchDatabaseIncludeManager) m = fsearch_database_index_store_get_include_manager(store);
    g_autoptr(GPtrArray) includes = fsearch_database_include_manager_get_includes(m);
    for (uint32_t i = 0; i < includes->len; i++) {
        fsearch_database_include_set_last_scan_time(g_ptr_array_index(includes, i), 0);
        fsearch_database_include_set_last_scan_duration(g_ptr_array_index(includes, i), 0);
    }
    return store;
}

static void
print_md5(const char *path) {
    g_autofree char *data = NULL;
    gsize len = 0;
    if (!g_file_get_contents(path, &data, &len, NULL)) {
        printf("md5 <unreadable>\n");
        return;
    }
    g_autofree char *sum = g_compute_checksum_for_data(G_CHECKSUM_MD5, (const guchar *)data, len);
    printf("bytes %" G_GSIZE_FORMAT " md5 %s\n", len, sum);
}

// scan, then print every entry sorted by path (scanner + folder size aggregation)
static int
cmd_scandump(int argc, char **argv) {
    g_autoptr(FsearchDatabaseIndexStore) store = scan_store(argc, argv);
    g_autoptr(FsearchQuery) q = fsearch_query_new("", NULL, NULL, 0, "dump");
    fsearch_database_index_store_search(store, 1, q, DATABASE_INDEX_PROPERTY_PATH, FSEARCH_SORT_DIRECTION_ASCENDING, NULL);
    print_view(store);
    return 0;
}

// scan, save with zeroed scan metadata, print size + md5 of the file (byte compatibility)
static int
cmd_scanmd5(int argc, char **argv) {
    g_autoptr(FsearchDatabaseIndexStore) store = scan_store(argc - 1, argv + 1);
    if (!fsearch_database_file_save(store, argv[0])) {
        printf("save failed\n");
        return 1;
    }
    print_md5(argv[0]);
    return 0;
}

// load a database and save it again
static int
cmd_resave(int argc, char **argv) {
    g_autoptr(FsearchDatabaseIndexStore) store = load(argv[0]);
    if (!fsearch_database_file_save(store, argv[1])) {
        printf("save failed\n");
        return 1;
    }
    print_md5(argv[1]);
    return 0;
}

static int
cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return x < y ? -1 : x > y;
}

static int
cmd_bench(int argc, char **argv) {
    const char *db = argv[0];
    int runs = atoi(argv[1]);
    FsearchDatabaseIndexProperty sort = parse_sort(argv[2]);
    FsearchFilterManager *filters = fsearch_filter_manager_new_with_defaults();
    double *t = calloc(runs, sizeof(double));
    // load
    for (int r = 0; r < runs; r++) {
        gint64 t0 = g_get_monotonic_time();
        FsearchDatabaseIndexStore *s = load_direct(db);
        t[r] = (g_get_monotonic_time() - t0) / 1000.0;
        fsearch_database_index_store_unref(s);
    }
    qsort(t, runs, sizeof(double), cmp_double);
    printf("load %.3f\n", t[runs / 2]);
    g_autoptr(FsearchDatabaseIndexStore) store = load_direct(db);
    for (int i = 3; i < argc; i++) {
        uint32_t n = 0;
        for (int r = 0; r < runs; r++) {
            g_autoptr(FsearchQuery) q = fsearch_query_new(argv[i], NULL, filters,
                                                          QUERY_FLAG_AUTO_MATCH_CASE | QUERY_FLAG_AUTO_SEARCH_IN_PATH,
                                                          "bench");
            gint64 t0 = g_get_monotonic_time();
            fsearch_database_index_store_search(store, 1, q, sort, FSEARCH_SORT_DIRECTION_ASCENDING, NULL);
            t[r] = (g_get_monotonic_time() - t0) / 1000.0;
            FsearchDatabaseSearchInfo *info = fsearch_database_index_store_get_search_info(store, 1);
            n = fsearch_database_search_info_get_num_entries(info);
            fsearch_database_search_info_unref(info);
        }
        qsort(t, runs, sizeof(double), cmp_double);
        printf("query %.3f %u %s\n", t[runs / 2], n, argv[i]);
    }
    // re-sort the full result set (match everything) by each property
    const char *sorts[] = {"path", "size", "mtime", "ext", "type"};
    for (int k = 0; k < 5; k++) {
        for (int r = 0; r < runs; r++) {
            g_autoptr(FsearchQuery) q = fsearch_query_new("", NULL, filters, 0, "bench");
            fsearch_database_index_store_search(store, 1, q, DATABASE_INDEX_PROPERTY_NAME,
                                                FSEARCH_SORT_DIRECTION_ASCENDING, NULL);
            gint64 t0 = g_get_monotonic_time();
            fsearch_database_index_store_sort_results(store, 1, parse_sort(sorts[k]),
                                                      FSEARCH_SORT_DIRECTION_ASCENDING, NULL);
            t[r] = (g_get_monotonic_time() - t0) / 1000.0;
        }
        qsort(t, runs, sizeof(double), cmp_double);
        printf("sort %.3f %s\n", t[runs / 2], sorts[k]);
    }
    free(t);
    return 0;
}

int
main(int argc, char **argv) {
    setlocale(LC_ALL, "");
    if (argc < 3) {
        fprintf(stderr, "usage: fsdriver scan|query|dump|bench ...\n");
        return 2;
    }
    if (!strcmp(argv[1], "scan")) return cmd_scan(argc - 2, argv + 2);
    if (!strcmp(argv[1], "query")) return cmd_query(argc - 2, argv + 2);
    if (!strcmp(argv[1], "dump")) return cmd_dump(argc - 2, argv + 2);
    if (!strcmp(argv[1], "bench")) return cmd_bench(argc - 2, argv + 2);
    if (!strcmp(argv[1], "scandump")) return cmd_scandump(argc - 2, argv + 2);
    if (!strcmp(argv[1], "scanmd5")) return cmd_scanmd5(argc - 2, argv + 2);
    if (!strcmp(argv[1], "resave")) return cmd_resave(argc - 2, argv + 2);
    fprintf(stderr, "unknown command\n");
    return 2;
}
