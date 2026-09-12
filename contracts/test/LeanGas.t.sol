// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {RiscZeroGroth16Verifier} from "../src/groth16/RiscZeroGroth16Verifier.sol";
import {ControlID} from "../src/groth16/ControlID.sol";
import {TheoremBounty, IRiscZeroVerifier} from "../src/TheoremBounty.sol";

/// Measure gas with a real receipt for Practical.thm_infinitude (infinitely many primes).
contract LeanGas is Test {
    bytes32 constant IMAGE_ID = 0x44088753d3612b3b5641daf6a7af9f962211854afe91693183d3719d1e1350c7;
    bytes32 constant JOURNAL_DIGEST = 0x31f44e449e0d6d44aac6c8502505335243a6e6ea85c31a84e61909607979808b;
    bytes constant SEAL = hex"73c457ba25aeef604aaceda946abf209c1dc9070c97b1a616a63708bec95f35fa75f23b02f4e722c34ce6305c910c6f44b0cb9de89ace7c5dbac4125d0c52751e0181e0d1a3819f7a9a7580b987b08611949cc17c0179f6eded10c20154c1f1dc0c28db205879249b12ddad02711b8cb859c25987cdce3e665e6620cef33b2c3b1b02f8b0817bbc22e5b3620f8744434470420171a1ac875834fc3002c53158c976c9f320fa6e8545cc95b629826b21f3a26c1d9fdb70fdbfb39cc70c57601e68e25fb611515993caa158599593215167e24a66f1703cf03c6f7ecab91772ef39e86fbb322c504795966f2b60a0fde7ba3423d136b530e55320aee891cfdeffc2c1d4e4c";

    RiscZeroGroth16Verifier verifier;
    TheoremBounty bounty;

    receive() external payable {}

    function setUp() public {
        verifier = new RiscZeroGroth16Verifier(ControlID.CONTROL_ROOT, ControlID.BN254_CONTROL_ID);
        bounty = new TheoremBounty{value: 1 ether}(
            IRiscZeroVerifier(address(verifier)), IMAGE_ID, JOURNAL_DIGEST
        );
    }

    function test_verify_only() public view {
        uint256 g = gasleft();
        verifier.verify(SEAL, IMAGE_ID, JOURNAL_DIGEST);
        console.log("verifier.verify  :", g - gasleft());
    }

    function test_claim() public {
        uint256 g = gasleft();
        bounty.claim(SEAL);
        console.log("bounty.claim     :", g - gasleft());
        assertEq(bounty.claimant(), address(this));
    }

    function test_calldata_size() public pure {
        bytes memory cd = abi.encodeWithSignature("claim(bytes)", SEAL);
        uint256 cost = 0;
        for (uint256 i = 0; i < cd.length; i++) {
            cost += cd[i] == 0 ? 4 : 16;
        }
        console.log("calldata bytes   :", cd.length);
        console.log("calldata gas     :", cost);
        console.log("intrinsic (21000):", 21000 + cost);
    }
}
