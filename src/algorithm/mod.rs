use itertools::Itertools;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand::seq::IteratorRandom;
use rayon::prelude::*;
use std::cmp::min;

use rand::Rng;

use self::{
    config::AlgorithmConfig,
    datatypes::{Chromosome, Individual, Population, Tuple},
    random::get_random_generator,
};

pub mod config;
pub mod datatypes;
mod random;

/// for each individual (list of periods) in population size
/// for tuple in tuples
/// assign tuple to a random period from individual
pub fn create_first_population(config: &AlgorithmConfig, tuples: &[Tuple]) -> Population {
    let AlgorithmConfig {
        population_size,
        number_of_periods,
        ..
    } = config.to_owned();

    let mut population = Population::with_capacity(population_size);

    let mut rng = get_random_generator();

    for _ in 0..population_size {
        let mut individual: Individual = Individual::new(number_of_periods);

        // create periods
        for period_id in 0..number_of_periods {
            let period = Chromosome::new(period_id.try_into().unwrap());

            individual.chromosomes.push(period);
        }

        // assign tuple to a random period from individual
        for tuple in tuples {
            let random_period_index = rng.gen_range(0..number_of_periods);
            individual.chromosomes[random_period_index]
                .genes
                .push(tuple.id);
        }

        population.push(individual)
    }

    population
}

pub fn rand_parents(parents: &Population) -> (&Individual, &Individual) {
    assert!(parents.len() > 1);

    let mut rng = get_random_generator();

    let sorted_parents = parents
        .into_iter()
        .sorted_by(|a, b| b.adaptation.partial_cmp(&a.adaptation).unwrap())
        .collect::<Vec<_>>();

    let weights = (0..sorted_parents.len())
        .map(|x| f64::exp((-0.3f64 * x as f64) + 2f64))
        .collect::<Vec<_>>();

    let dist = WeightedIndex::new(weights.clone()).unwrap();

    let idx1 = dist.sample(&mut rng);

    // Sample the second index ensuring its different from the first
    let idx2 = loop {
        let idx = dist.sample(&mut rng);
        if idx != idx1 {
            break idx;
        }
    };

    // println!(
    //     "Min: {}, Max: {}, Parent 1 weights: {}, Parent 2 weights: {}, Parent 1 weight: {}, Parent 2 weight: {}",
    //     min_adaptation, max_adaptation, p[idx1].adaptation, p[idx2].adaptation, weights[idx1], weights[idx2]
    // );

    return (
        sorted_parents.get(idx1).unwrap(),
        sorted_parents.get(idx2).unwrap(),
    );
}

pub fn crossover(config: &AlgorithmConfig, population: &Population) -> Individual {
    let AlgorithmConfig {
        number_of_periods, ..
    } = config.to_owned();

    let (mother, father) = rand_parents(population);

    let mut child: Individual = Individual::with_chromosomes(
        std::iter::zip(mother.chromosomes.iter(), father.chromosomes.iter())
            .collect::<Vec<_>>()
            .par_iter()
            // .par_bridge()
            .map(|(mother_chromosome, father_chromosome)| {
                assert_eq!(mother_chromosome.id, father_chromosome.id);
                let mut rng = get_random_generator();

                let id = mother_chromosome.id;

                let mother_genes = &father_chromosome.genes;
                let father_genes = &mother_chromosome.genes;

                let mating_point_upper_bound = min(mother_genes.len(), father_genes.len());

                let mating_point = rng.gen_range(0..=mating_point_upper_bound);

                let (mother_left, _) = mother_genes.split_at(mating_point);
                let (_, father_right) = father_genes.split_at(mating_point);
                let child_genes = mother_left
                    .iter()
                    .chain(father_right.iter())
                    .cloned()
                    .collect::<Vec<_>>();

                Chromosome {
                    id,
                    genes: child_genes,
                }
            })
            .collect(),
    );

    // at this point there could be duplicated and missing genes, so we want to fix this

    // repair lost
    let all_genes: Vec<i32> = mother
        .chromosomes
        .iter()
        .flat_map(|g| g.genes.iter().cloned())
        .collect();

    let lost_genes: Vec<i32> = all_genes
        .iter()
        .filter(|g| !child.chromosomes.iter().any(|c| c.genes.contains(g)))
        .cloned()
        .collect();

    let mut rng = get_random_generator();

    for gene in lost_genes {
        let period_id = rng.gen_range(0..number_of_periods);
        child.chromosomes[period_id].genes.push(gene);
    }

    // remove duplicates
    let mut seen = std::collections::HashSet::new();

    for period in &mut child.chromosomes {
        period.genes.retain(|x| seen.insert(x.clone()));
    }

    child
}

pub fn mutate(config: &AlgorithmConfig, individual: &mut Individual) {
    let mutation_probability = config.mutation_probability;
    let number_of_periods = usize::try_from(config.number_of_periods).unwrap();

    let mut rng = get_random_generator();

    for period_id in 0..number_of_periods {
        if rng.gen_bool(mutation_probability.into()) {
            let gene_count = individual.chromosomes[period_id].genes.len();

            if gene_count == 0 {
                continue;
            }

            let gene_index = rng.gen_range(0..gene_count);

            let gene = individual.chromosomes[period_id].genes.remove(gene_index);

            // remove gene from current period
            individual.chromosomes[period_id]
                .genes
                .retain(|g| g != &gene);

            // add gene to random period
            individual
                .chromosomes
                .iter_mut()
                .filter(|target| target.id != i32::try_from(period_id).unwrap())
                .choose(&mut rng)
                .unwrap()
                .genes
                .push(gene);
        }
    }
}

pub fn calculate_fitness(individual: &Individual, tuples: &Vec<Tuple>, debug: bool) -> i32 {
    let mut individual_fitness = 0;

    for period in &individual.chromosomes {
        // if teacher is teaching more than one class at the same time decrease fitness by 10

        let genes = &period.genes;

        for gene_id in genes {
            // if the same teacher is teaching more than one class at the same time decrease fitness by 10
            // if different teachers occupy the same room at the same time decrease fitness by 20
            // ToDo: consider splitting tuples lecture type, so CWL and LAB can be in the same room at the same time

            let tuple = tuples
                .iter()
                .find(|t| t.id == *gene_id)
                .expect(format!("Tuple with id {} not found", *gene_id).as_str());

            let this_room_classes = tuples
                .iter()
                .filter(|t| genes.contains(&t.id))
                .filter(|t| t.id != tuple.id)
                .filter(|t| t.room == tuple.room);

            // get count of tuples with the same teacher
            let same_teacher_different_classes_count = this_room_classes
                .clone()
                .filter(|t| t.teacher == tuple.teacher)
                .count();

            individual_fitness -= (same_teacher_different_classes_count as i32) * 10;

            let same_room_different_teacher_count = this_room_classes
                .clone()
                .filter(|t| t.teacher != tuple.teacher)
                .count();

            individual_fitness -= (same_room_different_teacher_count as i32) * 20;

            if debug {
                println!(
                    "same_teacher_different_classes_count: {}, same_room_different_teacher_count: {}",
                    same_teacher_different_classes_count, same_room_different_teacher_count
                );
            }
        }
    }

    if debug {
        println!("Individual fitness: {}", individual_fitness);
    }

    individual_fitness
}
