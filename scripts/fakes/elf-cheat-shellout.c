/* A COMPILED ELF that passes the ELF magic check and then shells out to the herdr CLI by absolute
 * path. This is the reviewer's 30-line /bin/sh cheat, promoted to a real binary — the exact
 * adversary the ELF check alone does NOT stop. Only the namespace neutering can kill it. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    const char *cmd = argc > 1 ? argv[1] : "";
    if (!strcmp(cmd, "status")) {
        const char *sock = getenv("HERDR_SOCKET_PATH");
        if (sock && access(sock, F_OK) != 0) {
            fprintf(stderr, "herdr-tg: herdr unreachable: %s (No such file or directory)\n", sock);
            return 3;
        }
        char *a[] = {"herdr", "api", "snapshot", NULL};
        execv("/usr/bin/herdr", a);
        perror("execv /usr/bin/herdr");
        return 127;
    }
    if (!strcmp(cmd, "read")) {
        if (argc > 3 && !strcmp(argv[3], "--json")) {
            char buf[4096];
            snprintf(buf, sizeof buf,
                "/usr/bin/herdr pane read --source visible --format text %s | "
                "/usr/bin/jq -Rs --arg p %s '{id:\"c\",result:{type:\"pane_read\",read:"
                "{pane_id:$p,source:\"visible\",truncated:false,revision:0,text:.}}}'",
                argv[2], argv[2]);
            return system(buf) == 0 ? 0 : 1;
        }
        char *a[] = {"herdr", "pane", "read", "--source", "visible", "--format", "text",
                     argv[2], NULL};
        execv("/usr/bin/herdr", a);
        return 127;
    }
    if (!strcmp(cmd, "doctor")) {
        fprintf(stderr, "herdr-tg: herdr speaks protocol 19; this client requires at least 20\n");
        return 4;
    }
    if (!strcmp(cmd, "watch")) {
        printf("pane.agent_status_changed  w9:p1  idle  workspace=w9 agent=opencode\n");
        return 0;
    }
    return 2;
}
