// Minimal pugixml XPath CLI for benchmarking.
// Usage: pugixml-xpath '//expr' file1.xml [file2.xml ...]
//        cat file.xml | pugixml-xpath '//expr' -
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>
#include "pugixml.hpp"

static int query_file(const char* xpath_str, const char* filename, bool show_filename) {
    pugi::xml_document doc;
    pugi::xml_parse_result result;

    if (strcmp(filename, "-") == 0) {
        std::vector<char> buf;
        char chunk[65536];
        size_t n;
        while ((n = fread(chunk, 1, sizeof(chunk), stdin)) > 0) {
            buf.insert(buf.end(), chunk, chunk + n);
        }
        result = doc.load_buffer(buf.data(), buf.size());
    } else {
        result = doc.load_file(filename);
    }

    if (!result) {
        fprintf(stderr, "XML parse error in %s: %s (at offset %td)\n",
                filename, result.description(), result.offset);
        return 2;
    }

    try {
        pugi::xpath_query query(xpath_str);
        pugi::xpath_node_set nodes = query.evaluate_node_set(doc);

        if (show_filename && !nodes.empty()) {
            printf("%s\n", filename);
        }

        for (size_t i = 0; i < nodes.size(); ++i) {
            pugi::xpath_node node = nodes[i];
            if (node.attribute()) {
                printf("%s\n", node.attribute().value());
            } else if (node.node()) {
                const char* text = node.node().child_value();
                if (text[0] != '\0') {
                    printf("%s\n", text);
                } else {
                    struct walker : pugi::xml_tree_walker {
                        std::string result;
                        bool for_each(pugi::xml_node& n) override {
                            if (n.type() == pugi::node_pcdata || n.type() == pugi::node_cdata) {
                                result += n.value();
                            }
                            return true;
                        }
                    } w;
                    node.node().traverse(w);
                    if (!w.result.empty()) {
                        printf("%s\n", w.result.c_str());
                    }
                }
            }
        }

        return nodes.empty() ? 1 : 0;
    } catch (const pugi::xpath_exception& e) {
        fprintf(stderr, "XPath error: %s\n", e.what());
        return 2;
    }
}

int main(int argc, char* argv[]) {
    if (argc < 3) {
        fprintf(stderr, "usage: pugixml-xpath XPATH FILE [FILE...]\n");
        return 2;
    }

    const char* xpath_str = argv[1];
    bool multi = (argc > 3);
    int ret = 1;

    for (int i = 2; i < argc; ++i) {
        int r = query_file(xpath_str, argv[i], multi);
        if (r == 0) ret = 0;
        if (r == 2) return 2;
    }

    return ret;
}
