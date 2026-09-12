// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

/// RISC Zero's verifier contract, already deployed on each chain.
/// Reverts on failure; there is no return value.
interface IRiscZeroVerifier {
    function verify(bytes calldata seal, bytes32 imageId, bytes32 journalDigest) external view;
}

/// Show on chain that Lean's kernel accepted a proposition, **without revealing the
/// proof term**.
///
/// All that is passed is a 260-byte seal. Even the journal can stay off chain.
///
/// ## Fixed at deploy time
///
/// - `imageId`       which guest: this pins the kernel (nanoda) and the checking
///                   policy (the permitted axioms)
/// - `journalDigest` a digest of the whole journal. This one value pins **the prelude
///                   SHA-256, the record root, the declaration-name root, the theorem
///                   name, the statement digest and the set of skipped axioms**
///
/// The journal contains variable-length `String` and `Vec<String>` under risc0-serde,
/// which is awkward to parse in Solidity. It is deterministic, so it is **folded into
/// a single digest and baked in at deploy time** — at the cost of one deployment per
/// proposition.
///
/// ## The assumption does not change on chain
///
/// `journalDigest` carries a commitment to the prelude, so *which* environment was
/// assumed is pinned. But **that the prelude type-checks remains an assumption**
/// (`assumed` mode). Discharging it needs a Stage 1 receipt: about 540 billion cycles
/// at Mathlib scale.
///
/// ## Known hole: front-running
///
/// The seal appears in calldata, so a third party watching the mempool can call
/// `claim` first with the same seal. Closing this requires **putting the claimant's
/// address in the journal** (the guest taking an address as input and committing to
/// it), but then the journal varies per claimant and the deploy-time digest used here
/// no longer works. The choice is commit-reveal, or parsing the journal.
contract TheoremBounty {
    IRiscZeroVerifier public immutable verifier;
    bytes32 public immutable imageId;
    bytes32 public immutable journalDigest;

    address public claimant;

    event Claimed(address indexed who, uint256 reward);

    constructor(IRiscZeroVerifier _verifier, bytes32 _imageId, bytes32 _journalDigest) payable {
        verifier = _verifier;
        imageId = _imageId;
        journalDigest = _journalDigest;
    }

    /// The proof term (3 MB here) is never passed. Only the 260-byte seal is.
    function claim(bytes calldata seal) external {
        require(claimant == address(0), "already claimed");
        verifier.verify(seal, imageId, journalDigest);
        claimant = msg.sender;
        uint256 reward = address(this).balance;
        emit Claimed(msg.sender, reward);
        (bool ok, ) = msg.sender.call{value: reward}("");
        require(ok, "transfer failed");
    }

    /// Anyone can verify without changing state.
    function check(bytes calldata seal) external view {
        verifier.verify(seal, imageId, journalDigest);
    }
}
