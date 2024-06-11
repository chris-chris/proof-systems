#!/bin/sh
# Test preimage read from VM request to server response
# Usage: test_preimage_read.sh [OP_DB_DIR] [NETWORK_NAME]

source rpcs.sh

set +u
if [ -z "${FILENAME}" ]; then
    FILENAME="$(./setenv-for-latest-l2-block.sh)"
fi
set -u

source $FILENAME

set -x

# Location of prepopulated OP DB
# OP_PROGRAM_DATA_DIR is generated by ./setenv-for-latest-block.sh
# It makes sense to use it as default value here
DATADIR=${1:-${OP_PROGRAM_DATA_DIR}}
NETWORK=${2:-sepolia}

# If you actually need to populate the OP DB, run
#################################################
# ${OP_DIR}/op-program/bin/op-program \         #
#     --log.level DEBUG \                       #
#     --l1 ${L1_RPC} \                          #
#     --l2 ${L2_RPC} \                          #
#     --network ${NETWORK} \                    #
#     --datadir ${DATADIR} \                    #
#     --l1.head ${L1_HEAD} \                    #
#     --l2.head ${L2_HEAD} \                    #
#     --l2.outputroot ${STARTING_OUTPUT_ROOT} \ #
#     --l2.claim ${L2_CLAIM} \                  #
#     --l2.blocknumber ${L2_BLOCK_NUMBER}       #
#################################################

# Run test with debug on
RUST_LOG=debug cargo run -r --bin test_optimism_preimage_read -- \
    --preimage-db-dir ${DATADIR} -- \
    op-program \
    --log.level DEBUG \
    --l1 ${L1_RPC} \
    --l2 ${L2_RPC} \
    --network ${NETWORK} \
    --datadir ${DATADIR} \
    --l1.head ${L1_HEAD} \
    --l2.head ${L2_HEAD} \
    --l2.outputroot ${STARTING_OUTPUT_ROOT} \
    --l2.claim ${L2_CLAIM} \
    --l2.blocknumber ${L2_BLOCK_NUMBER} \
    --server
