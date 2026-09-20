import argparse
import json
import random
import subprocess
import time


def failure_restarts(validators, ssh_key, num_concurrent):
    """
    Restarts multiple validators at a time. Randomly sleeps between 1 to 5 mins
    to allow the nodes come up.
    """
    i = 0
    # Loop through all validators
    while i < len(validators):
        j = 0
        # Restart num_concurrent validators
        while j < num_concurrent:
            print(f"Restarting validator {validators[i + j]}")
            subprocess.check_output(
                f"ssh -i {ssh_key} ubuntu@{validators[i + j]} 'sudo -S -p \"\" systemctl restart paxd'")
            j += 1
        i += num_concurrent

        time.sleep(random.randint(60, 300))


def delegation_change(chain_id):
    """
    Changes delegation of validators to create imbalanced delegation. Delegates
    10 * total to the top 4 validators for a period of time between 10 minutes
    to 1 hour.
    """
    # First get total voting power
    admin_acc = _get_admin_acc()
    paxd_query_cmd = f"paxd q staking delegations {admin_acc} --chain-id {chain_id} --output json"
    staking_output = json.loads(
        subprocess.check_output([paxd_query_cmd], stderr=subprocess.STDOUT,
                                shell=True))
    total_delegation = 0
    validator_addrs = []
    for resp in staking_output["delegation_responses"]:
        total_delegation += int(resp["balance"]["amount"])
        validator_addrs.append(resp["delegation"]["validator_address"])
    # Delegate 20x to 1/4 of the validators
    delegations = {}
    for i in range(len(validator_addrs // 4)):
        delegations[validator_addrs[i]] = 10 * total_delegation
        print(f"Delegating {10 * total_delegation} to {validator_addrs[i]}")
        paxd_staking_cmd = f"paxd tx staking delegate {validator_addrs[i]} {10 * total_delegation}uhpx --from admin --chain-id {chain_id} -b block -y"
        _run_paxd_cmd(paxd_staking_cmd)
    time.sleep(random.randint(600, 3600))
    # Unbond
    for validator in delegations:
        print(f"Unbonding {10 * total_delegation} from {validator}")
        paxd_unbond_cmd = f"paxd tx staking unbond {validator} {delegations[validator]}uhpx --from admin -b block -y --chain-id {chain_id}"
        _run_paxd_cmd(paxd_unbond_cmd)


def slow_network(validators, ssh_key, num_concurrent):
    """
    Slows the network of multiple validators at a time by adding 200ms latency.
    Removes the latency after 10 mins to 1 hour.
    """
    i = 0
    # Loop through all validators
    while i < len(validators):
        j = 0
        slowed_validators = []
        # Restart num_concurrent validators
        while j < num_concurrent:
            print(f"Slowing validators {validators[i + j]} by adding tc rules")
            slowed_validators.append(validators[i + j])
            tc_cmd = f"ssh -i {ssh_key} ubuntu@{validators[i + j]} 'sudo -S -p \"\" tc qdisc add dev ens5 root netem delay 200ms 10ms 25%'"
            subprocess.check_output(tc_cmd, stderr=subprocess.STDOUT,
                                    shell=True)
            j += 1
        time.sleep(random.randint(600, 3600))
        print(f"Removing tc rules from validators {slowed_validators}")
        for validator in slowed_validators:
            tc_cmd = f"ssh -i {ssh_key} ubuntu@{validator} 'sudo -S -p \"\" tc qdisc del dev ens5 root'"
            subprocess.check_output(tc_cmd, stderr=subprocess.STDOUT,
                                    shell=True)
        i += num_concurrent


def _get_admin_acc():
    paxd_query_cmd = "paxd keys list --output json"
    accs_output = _run_paxd_cmd(paxd_query_cmd)
    return filter(lambda x: x['name'] == 'admin', accs_output)['address']

def _run_paxd_cmd(cmd)
    paxd_cmd = "printf '12345678\n' " + cmd
    output = json.loads(
        subprocess.check_output([paxd_cmd], stderr=subprocess.STDOUT,
                                shell=True))
    return output

def main():
    parser = argparse.ArgumentParser(
        description="Runs various failure scenarios on validators. Note that delegation change must be run on genesis validator")
    parser.add_argument('-s', '--ssh-key', type=str,
                        help='Ssh key for validator nodes')
    parser.add_argument('-f', '--failure', type=str, choices=["restart", "delegation-change", "slow-network"],
                        help='Type of failure to run. Should be one of ["restart", "delegation-change", "slow-network"]')
    parser.add_argument('-v', '--validators', type=str,
                        help='Comma separated list of validator nodes')
    parser.add_argument('-c', '--chain-id', type=str, help='Chain id')
    args = parser.parse_args()
    if args.failure == "restart":
        failure_restarts(args.validators.split(','), args.ssh_key, 2)
    elif args.failure == "delegation-change":
        delegation_change(args.chain_id)
    elif args.failure == "slow-network":
        slow_network(args.validators.split(','), args.ssh_key, 2)


if __name__ == "__main__":
    main()
