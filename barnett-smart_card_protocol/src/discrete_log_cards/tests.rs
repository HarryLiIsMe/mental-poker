#[cfg(test)]
mod test {
    use crate::discrete_log_cards;
    use crate::error::CardProtocolError;
    use crate::BarnettSmartProtocol;

    use ark_ff::UniformRand;
    use ark_std::{rand::Rng, Zero};
    use crypto_primitives::error::CryptoError;
    use crypto_primitives::utils::permutation::Permutation;
    use crypto_primitives::utils::rand::sample_vector;
    use crypto_primitives::zkp::proofs::schnorr_identification;
    use crypto_primitives::zkp::ArgumentOfKnowledge;
    use rand::thread_rng;
    use std::iter::Iterator;

    // Choose elliptic curve setting
    type Curve = starknet_curve::Projective;
    type Scalar = starknet_curve::Fr;

    // Instantiate concrete type for our card protocol
    type CardProtocol<'a> = discrete_log_cards::DLCards<'a, Curve>;
    type CardParameters = discrete_log_cards::Parameters<Curve>;
    type PublicKey = discrete_log_cards::PublicKey<Curve>;
    type SecretKey = discrete_log_cards::PlayerSecretKey<Curve>;

    type Card = discrete_log_cards::Card<Curve>;
    type MaskedCard = discrete_log_cards::MaskedCard<Curve>;
    type RevealToken = discrete_log_cards::RevealToken<Curve>;

    type KeyOwnArg<'a> = schnorr_identification::SchnorrIdentification<Curve>;
    type ProofKeyOwnership = schnorr_identification::proof::Proof<Curve>;

    fn setup_players<R: Rng>(
        rng: &mut R,
        parameters: &CardParameters,
        num_of_players: usize,
    ) -> (Vec<(PublicKey, SecretKey)>, PublicKey) {
        let mut players: Vec<(PublicKey, SecretKey)> = Vec::with_capacity(num_of_players);
        let mut expected_shared_key = PublicKey::zero();

        for i in 0..num_of_players {
            players.push(CardProtocol::player_keygen(rng, &parameters).unwrap());
            expected_shared_key = expected_shared_key + players[i].0
        }

        (players, expected_shared_key)
    }

    #[test]
    fn generate_and_verify_key() {
        let rng = &mut thread_rng();
        let m = 4;
        let n = 13;

        let parameters = CardProtocol::setup(rng, m, n).unwrap();

        let (pk, sk) = CardProtocol::player_keygen(rng, &parameters).unwrap();

        let p1_keyproof = CardProtocol::prove_key_ownership(rng, &parameters, &pk, &sk).unwrap();

        assert_eq!(
            Ok(()),
            p1_keyproof.verify(&parameters.enc_parameters.generator, &pk)
        );

        let other_key = Scalar::rand(rng);
        let wrong_proof =
            CardProtocol::prove_key_ownership(rng, &parameters, &pk, &other_key).unwrap();

        assert_eq!(
            wrong_proof.verify(&parameters.enc_parameters.generator, &pk),
            Err(CryptoError::ProofVerificationError(String::from(
                "Schnorr Identification"
            )))
        )
    }

    #[test]
    fn aggregate_keys() {
        let rng = &mut thread_rng();
        let m = 4;
        let n = 13;

        let num_of_players = 10;

        let parameters = CardProtocol::setup(rng, m, n).unwrap();

        let (players, expected_shared_key) = setup_players(rng, &parameters, num_of_players);

        let proofs = players
            .iter()
            .map(|player| {
                KeyOwnArg::prove(
                    rng,
                    &parameters.enc_parameters.generator,
                    &player.0,
                    &player.1,
                )
                .unwrap()
            })
            .collect::<Vec<ProofKeyOwnership>>();

        let key_proof_pairs = players
            .iter()
            .zip(proofs.iter())
            .map(|(player, &proof)| (player.0, proof.clone()))
            .collect::<Vec<(PublicKey, ProofKeyOwnership)>>();

        let test_aggregate =
            CardProtocol::compute_aggregate_key(&parameters, &key_proof_pairs).unwrap();

        assert_eq!(test_aggregate, expected_shared_key);

        let mut bad_key_proof_pairs = key_proof_pairs;
        bad_key_proof_pairs[0].0 = PublicKey::zero();

        let test_fail_aggregate =
            CardProtocol::compute_aggregate_key(&parameters, &bad_key_proof_pairs);

        assert_eq!(
            test_fail_aggregate,
            Err(CardProtocolError::ProofVerificationError(
                CryptoError::ProofVerificationError(String::from("Schnorr Identification"))
            ))
        )
    }

    #[test]
    fn test_unmask() {
        let rng = &mut thread_rng();
        let m = 4;
        let n = 13;

        let num_of_players = 10;

        let parameters = CardProtocol::setup(rng, m, n).unwrap();

        let (players, expected_shared_key) = setup_players(rng, &parameters, num_of_players);

        let card = Card::rand(rng);
        let alpha = Scalar::rand(rng);
        let (masked, _) =
            CardProtocol::mask(rng, &parameters, &expected_shared_key, &card, &alpha).unwrap();

        let decryption_key = players
            .iter()
            .map(|player| {
                let (token, proof) = CardProtocol::compute_reveal_token(
                    rng,
                    &parameters,
                    &player.1,
                    &player.0,
                    &masked,
                )
                .unwrap();

                (token, proof, player.0)
            })
            .collect::<Vec<_>>();

        let unmasked = CardProtocol::unmask(&parameters, &decryption_key, &masked).unwrap();

        assert_eq!(card, unmasked);

        let mut bad_decryption_key = decryption_key;
        bad_decryption_key[0].0 = RevealToken::rand(rng);

        let failed_decryption = CardProtocol::unmask(&parameters, &bad_decryption_key, &masked);

        assert_eq!(
            failed_decryption,
            Err(CardProtocolError::ProofVerificationError(
                CryptoError::ProofVerificationError(String::from("Chaum-Pedersen"))
            ))
        )
    }

    #[test]
    fn test_shuffle() {
        let rng = &mut thread_rng();
        let m = 4;
        let n = 13;

        let num_of_players = 10;

        let parameters = CardProtocol::setup(rng, m, n).unwrap();

        let (_, aggregate_key) = setup_players(rng, &parameters, num_of_players);

        let deck: Vec<MaskedCard> = sample_vector(rng, m * n);

        let permutation = Permutation::new(rng, m * n);
        let masking_factors: Vec<Scalar> = sample_vector(rng, m * n);

        let (shuffled_deck, shuffle_proof) = CardProtocol::shuffle_and_remask(
            rng,
            &parameters,
            &aggregate_key,
            &deck,
            &masking_factors,
            &permutation,
        )
        .unwrap();

        assert_eq!(
            Ok(()),
            CardProtocol::verify_shuffle(
                &parameters,
                &aggregate_key,
                &deck,
                &shuffled_deck,
                &shuffle_proof
            )
        );

        let wrong_output: Vec<MaskedCard> = sample_vector(rng, m * n);

        assert_eq!(
            CardProtocol::verify_shuffle(
                &parameters,
                &aggregate_key,
                &deck,
                &wrong_output,
                &shuffle_proof
            ),
            Err(CryptoError::ProofVerificationError(String::from(
                "Hadamard Product (5.1)"
            )))
        )
    }
}
