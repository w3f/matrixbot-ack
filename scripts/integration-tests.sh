#!/bin/bash

source /scripts/common.sh
source /scripts/bootstrap-helm.sh


run_tests() {
    echo Running tests...

    wait_pod_ready matrixbot-escalation
    wait_pod_ready matrixbot-alert
}

main(){
    /scripts/build-helmfile.sh

    run_tests
}

main
